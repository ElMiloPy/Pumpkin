use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::{EntityStatus, EntityType};
use pumpkin_data::sound::Sound;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::Difficulty;

use crate::entity::ai::goal::{
    Controls, Goal, active_target::ActiveTargetGoal, look_around::RandomLookAroundGoal,
    look_at_entity::LookAtEntityGoal, swim::SwimGoal, wander_around::WanderAroundGoal,
};
use crate::entity::custom_sound::CustomSound;
use crate::entity::mob::{Mob, MobEntity};
use crate::entity::{Entity, EntityBase};

pub struct GuardianAttackGoal {
    _mob: Weak<dyn Mob>,
    attack_time: i32,
    elder: bool,
}

impl GuardianAttackGoal {
    #[must_use]
    pub const fn new(mob: Weak<dyn Mob>, elder: bool) -> Self {
        Self {
            _mob: mob,
            attack_time: -10,
            elder,
        }
    }
}

impl Goal for GuardianAttackGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        let target = mob.get_mob_entity().get_target().clone();
        target.is_some_and(|t| t.get_entity().is_alive())
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        let target = mob.get_mob_entity().get_target().clone();
        if let Some(target) = target {
            if !target.get_entity().is_alive() {
                return false;
            }
            if self.elder {
                return true;
            }
            let mob_pos = mob.get_entity().pos.load();
            let target_pos = target.get_entity().pos.load();
            mob_pos.squared_distance_to_vec(&target_pos) > 9.0
        } else {
            false
        }
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.attack_time = -10;
        mob.get_mob_entity().steered().navigator().stop();
        if let Some(target) = mob.get_mob_entity().get_target().clone() {
            mob.get_mob_entity()
                .look_control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .look_at_entity_with_range(&target, 90.0, 90.0);
        }
    }

    fn stop(&mut self, mob: &dyn Mob) {
        mob.get_entity().set_synced_data(
            pumpkin_data::tracked_data::guardian::DATA_ID_ATTACK_TARGET,
            VarInt(0),
        );
        mob.get_mob_entity().set_target(None);
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        Controls::MOVE | Controls::LOOK
    }

    fn tick(&mut self, mob: &dyn Mob) {
        let Some(target) = mob.get_mob_entity().get_target().clone() else {
            return;
        };

        mob.get_mob_entity().steered().navigator().stop();
        mob.get_mob_entity()
            .look_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .look_at_entity_with_range(&target, 90.0, 90.0);

        if !mob.has_line_of_sight(target.get_entity()) {
            mob.get_mob_entity().set_target(None);
            return;
        }

        self.attack_time += 1;
        if self.attack_time == 0 {
            let target_id = target.get_entity().entity_id;
            mob.get_entity().set_synced_data(
                pumpkin_data::tracked_data::guardian::DATA_ID_ATTACK_TARGET,
                VarInt(target_id),
            );
            if !mob.get_entity().is_silent() {
                let world = mob.get_entity().world.load_full();
                world.send_entity_status(mob.get_entity(), EntityStatus::GuardianAttackSound, None);
            }
        } else if self.attack_time >= 80 {
            let entity = mob.get_entity();
            let world = entity.world.load();
            let mut magic_damage = 1.0f32;
            if world.level_info.load().difficulty == Difficulty::Hard {
                magic_damage += 2.0;
            }
            if self.elder {
                magic_damage += 2.0;
            }

            let attack_damage =
                mob.get_mob_entity()
                    .living_entity
                    .get_attribute_value(&Attributes::ATTACK_DAMAGE) as f32;

            let _ = target.damage(
                entity,
                magic_damage + attack_damage,
                DamageType::INDIRECT_MAGIC,
            );

            mob.get_mob_entity().set_target(None);
        }
    }
}

pub struct GuardianEntity {
    pub mob_entity: MobEntity,
}

impl GuardianEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let guardian = Self { mob_entity };
        let mob_arc = Arc::new(guardian);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;

            goal_selector.add_goal(0, Box::new(SwimGoal::default()));
            goal_selector.add_goal(
                1,
                Box::new(GuardianAttackGoal::new(mob_weak.clone(), false)),
            );
            goal_selector.add_goal(4, Box::new(WanderAroundGoal::new(1.0)));
            goal_selector.add_goal(
                5,
                LookAtEntityGoal::with_default(mob_weak.clone(), &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(6, Box::new(RandomLookAroundGoal::default()));

            let target_selector = &mob_arc.mob_entity.target_selector;
            target_selector.add_goal(
                1,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::PLAYER, true),
            );
            target_selector.add_goal(
                2,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::SQUID, true),
            );
            target_selector.add_goal(
                2,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::GLOW_SQUID, true),
            );
            target_selector.add_goal(
                3,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::AXOLOTL, true),
            );
        };

        mob_arc
    }
}

impl CustomSound for GuardianEntity {
    fn hurt_sound(&self) -> Option<Sound> {
        let entity = self.get_entity();
        let is_water = entity.touching_water.load(Ordering::Relaxed);
        Some(if is_water {
            Sound::EntityGuardianHurt
        } else {
            Sound::EntityGuardianHurtLand
        })
    }

    fn death_sound(&self) -> Option<Sound> {
        let entity = self.get_entity();
        let is_water = entity.touching_water.load(Ordering::Relaxed);
        Some(if is_water {
            Sound::EntityGuardianDeath
        } else {
            Sound::EntityGuardianDeathLand
        })
    }
}

impl Mob for GuardianEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn on_damage(&self, _damage_type: DamageType, source: Option<&dyn EntityBase>) {
        if let Some(src) = source
            && let Some(living) = src.get_living_entity()
        {
            let _ = living.damage(src, 2.0, DamageType::THORNS);
        }
    }
}
