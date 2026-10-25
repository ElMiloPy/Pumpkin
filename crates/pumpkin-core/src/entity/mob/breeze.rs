use std::sync::{Arc, Weak};

use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::{Sound, SoundCategory};

use crate::entity::{
    Entity, EntityBase,
    ai::goal::{
        active_target::ActiveTargetGoal, look_around::RandomLookAroundGoal,
        look_at_entity::LookAtEntityGoal, ranged_attack::RangedAttackGoal, swim::SwimGoal,
        wander_around::WanderAroundGoal,
    },
    custom_sound::CustomSound,
    mob::{Mob, MobEntity, RangedAttackMob},
    projectile::{
        ThrownItemEntity,
        wind_charge::{WIND_CHARGE_GRAVITY, WindChargeEntity},
    },
};

pub struct BreezeEntity {
    pub mob_entity: MobEntity,
}

impl BreezeEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let breeze = Self { mob_entity };
        let mob_arc = Arc::new(breeze);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };
        let ranged_weak: Weak<dyn RangedAttackMob> = {
            let ranged_arc: Arc<dyn RangedAttackMob> = mob_arc.clone();
            Arc::downgrade(&ranged_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;

            goal_selector.add_goal(0, Box::new(SwimGoal::default()));
            goal_selector.add_goal(
                1,
                Box::new(RangedAttackGoal::new(ranged_weak, 1.0, 40, 24.0)),
            );
            goal_selector.add_goal(5, Box::new(WanderAroundGoal::new(1.0)));
            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak.clone(), &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(7, Box::new(RandomLookAroundGoal::default()));

            let target_selector = &mob_arc.mob_entity.target_selector;
            target_selector.add_goal(
                1,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::PLAYER, true),
            );
        };

        mob_arc
    }
}

impl CustomSound for BreezeEntity {
    fn death_sound(&self) -> Option<Sound> {
        Some(Sound::EntityBreezeDeath)
    }

    fn hurt_sound(&self) -> Option<Sound> {
        Some(Sound::EntityBreezeHurt)
    }
}

impl Mob for BreezeEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn pre_damage(&self, damage_type: DamageType, _source: Option<&dyn EntityBase>) -> bool {
        if damage_type == DamageType::ARROW
            || damage_type == DamageType::MOB_PROJECTILE
            || damage_type == DamageType::THROWN
            || damage_type == DamageType::TRIDENT
            || damage_type == DamageType::WIND_CHARGE
        {
            let entity = &self.mob_entity.living_entity.entity;
            let world = entity.world.load();
            world.play_sound(
                Sound::EntityBreezeDeflect,
                SoundCategory::Hostile,
                &entity.pos.load(),
            );
            return false;
        }
        true
    }
}

impl RangedAttackMob for BreezeEntity {
    fn perform_ranged_attack(&self, target: &Arc<dyn EntityBase>, _power: f32) {
        let entity = &self.mob_entity.living_entity.entity;
        let world = entity.world.load_full();

        let breeze_pos = entity.pos.load();
        let target_pos = target.get_entity().pos.load();

        let dx = target_pos.x - breeze_pos.x;
        let dy = (target_pos.y + target.get_entity().get_eye_height() * 0.5)
            - (breeze_pos.y + entity.get_eye_height());
        let dz = target_pos.z - breeze_pos.z;

        let thrown = ThrownItemEntity::new(
            Entity::new(world.clone(), breeze_pos, &EntityType::BREEZE_WIND_CHARGE),
            entity,
            WIND_CHARGE_GRAVITY,
        );
        thrown.set_velocity(dx, dy, dz, 1.5, 1.0);

        let wind_charge = WindChargeEntity::new_breeze(thrown);
        world.spawn_entity(Arc::new(wind_charge));
        world.play_sound(
            Sound::EntityBreezeShoot,
            SoundCategory::Hostile,
            &breeze_pos,
        );
    }
}
