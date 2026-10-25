use std::sync::{Arc, Weak};

use crossbeam::atomic::AtomicCell;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_util::math::vector3::Vector3;

use crate::entity::Entity;
use crate::entity::ai::goal::active_target::ActiveTargetGoal;
use crate::entity::ai::goal::{
    Controls, Goal, look_around::RandomLookAroundGoal, look_at_entity::LookAtEntityGoal,
};
use crate::entity::custom_sound::CustomSound;
use crate::entity::mob::{Mob, MobEntity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AttackPhase {
    Circle,
    Swoop,
}

struct PhantomFlightGoal {
    phantom: Weak<PhantomEntity>,
    next_sweep_ticks: i32,
    angle: f32,
    distance: f32,
    height: f32,
    clockwise: f32,
}

impl PhantomFlightGoal {
    pub fn new(phantom: Weak<PhantomEntity>) -> Self {
        Self {
            phantom,
            next_sweep_ticks: rand::random_range(160..240),
            angle: 0.0,
            distance: 10.0,
            height: 0.0,
            clockwise: 1.0,
        }
    }
}

impl Goal for PhantomFlightGoal {
    fn can_start(&mut self, _mob: &dyn Mob) -> bool {
        true
    }

    fn should_continue(&mut self, _mob: &dyn Mob) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        Controls::MOVE
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, _mob: &dyn Mob) {
        let Some(phantom) = self.phantom.upgrade() else {
            return;
        };

        let entity = &phantom.mob_entity.living_entity.entity;
        let pos = entity.pos.load();
        let target_opt = phantom.mob_entity.get_target().clone();

        let phase = phantom.attack_phase.load();
        let target_pos = match phase {
            AttackPhase::Circle => {
                self.next_sweep_ticks -= 1;
                if self.next_sweep_ticks <= 0 && target_opt.is_some() {
                    phantom.attack_phase.store(AttackPhase::Swoop);
                    self.next_sweep_ticks = rand::random_range(160..240);
                    let world = entity.world.load();
                    world.play_sound_fine(
                        Sound::EntityPhantomSwoop,
                        SoundCategory::Hostile,
                        &pos,
                        1.0,
                        rand::random::<f32>() * 0.2 + 0.9,
                    );
                }

                let anchor = target_opt.as_ref().map_or_else(
                    || phantom.anchor_point.load(),
                    |target| target.get_entity().pos.load().add_raw(0.0, 18.0, 0.0),
                );

                self.angle += self.clockwise * 0.08;
                let target_x =
                    f64::from(self.distance).mul_add(f64::from(self.angle.cos()), anchor.x);
                let target_y = anchor.y + f64::from(self.height);
                let target_z =
                    f64::from(self.distance).mul_add(f64::from(self.angle.sin()), anchor.z);
                Vector3::new(target_x, target_y, target_z)
            }
            AttackPhase::Swoop => target_opt.as_ref().map_or_else(
                || {
                    phantom.attack_phase.store(AttackPhase::Circle);
                    pos.add_raw(0.0, 10.0, 0.0)
                },
                |target| {
                    let t_pos = target.get_entity().pos.load();
                    let dist_sq = pos.squared_distance_to_vec(&t_pos);
                    if dist_sq < 4.0 {
                        let attack_damage = phantom
                            .mob_entity
                            .living_entity
                            .get_attribute_value(&Attributes::ATTACK_DAMAGE)
                            as f32;
                        let _ =
                            target.damage(entity, attack_damage.max(6.0), DamageType::MOB_ATTACK);
                        let world = entity.world.load();
                        world.play_sound_fine(
                            Sound::EntityPhantomBite,
                            SoundCategory::Hostile,
                            &pos,
                            1.0,
                            1.0,
                        );
                        phantom.attack_phase.store(AttackPhase::Circle);
                        phantom.anchor_point.store(t_pos.add_raw(0.0, 18.0, 0.0));
                    }
                    t_pos
                },
            ),
        };

        let dx = target_pos.x - pos.x;
        let dy = target_pos.y - pos.y;
        let dz = target_pos.z - pos.z;
        let dist = dx.hypot(dz).hypot(dy);
        if dist > 0.001 {
            let speed = if phase == AttackPhase::Swoop {
                0.35
            } else {
                0.18
            };
            let desired = Vector3::new(dx / dist * speed, dy / dist * speed, dz / dist * speed);
            let cur_vel = entity.velocity.load();
            let new_vel = Vector3::new(
                (desired.x - cur_vel.x).mul_add(0.2, cur_vel.x),
                (desired.y - cur_vel.y).mul_add(0.2, cur_vel.y),
                (desired.z - cur_vel.z).mul_add(0.2, cur_vel.z),
            );
            entity.velocity.store(new_vel);

            let yaw = (new_vel.z).atan2(new_vel.x).to_degrees() as f32 - 90.0;
            let pitch = (-(new_vel.y).atan2(new_vel.horizontal_length())).to_degrees() as f32;
            entity.set_rotation(yaw, pitch);
        }
    }
}

pub struct PhantomEntity {
    pub mob_entity: MobEntity,
    pub attack_phase: AtomicCell<AttackPhase>,
    pub anchor_point: AtomicCell<Vector3<f64>>,
}

impl PhantomEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let spawn_pos = entity.pos.load();
        let mob_entity = MobEntity::new(entity);
        let phantom = Self {
            mob_entity,
            attack_phase: AtomicCell::new(AttackPhase::Circle),
            anchor_point: AtomicCell::new(spawn_pos.add_raw(0.0, 18.0, 0.0)),
        };
        let mob_arc = Arc::new(phantom);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;
            let target_selector = &mob_arc.mob_entity.target_selector;

            goal_selector.add_goal(
                1,
                Box::new(PhantomFlightGoal::new(Arc::downgrade(&mob_arc))),
            );
            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak, &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(6, Box::new(RandomLookAroundGoal::default()));

            target_selector.add_goal(
                1,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::PLAYER, true),
            );
        };

        mob_arc
    }
}

impl CustomSound for PhantomEntity {
    fn hurt_sound(&self) -> Option<Sound> {
        Some(Sound::EntityPhantomHurt)
    }

    fn death_sound(&self) -> Option<Sound> {
        Some(Sound::EntityPhantomDeath)
    }

    fn sound_category(&self) -> SoundCategory {
        SoundCategory::Hostile
    }
}

impl Mob for PhantomEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn get_mob_gravity(&self) -> f64 {
        0.0
    }

    fn as_custom_sound(&self) -> Option<&dyn CustomSound> {
        Some(self)
    }
}
