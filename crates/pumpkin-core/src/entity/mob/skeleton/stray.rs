use std::sync::Arc;

use pumpkin_data::sound::{Sound, SoundCategory};

use crate::entity::{
    Entity,
    custom_sound::CustomSound,
    mob::{Mob, MobEntity, skeleton::SkeletonEntityBase, spawn::SpawnGroupData},
    projectile::arrow::ArrowEntity,
};
use crate::world::World;

pub struct StraySkeletonEntity {
    entity: Arc<SkeletonEntityBase>,
}

impl StraySkeletonEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = SkeletonEntityBase::new(entity);
        let stray = Self { entity };
        Arc::new(stray)
    }
}

impl CustomSound for StraySkeletonEntity {
    fn death_sound(&self) -> Option<Sound> {
        Some(Sound::EntityStrayDeath)
    }

    fn hurt_sound(&self) -> Option<Sound> {
        Some(Sound::EntityStrayHurt)
    }

    fn sound_category(&self) -> SoundCategory {
        SoundCategory::Hostile
    }
}

impl Mob for StraySkeletonEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn as_custom_sound(&self) -> Option<&dyn CustomSound> {
        Some(self)
    }

    fn shoot_sound(&self) -> Option<Sound> {
        Some(Sound::EntitySkeletonShoot)
    }

    fn get_arrow(&self, arrow: ArrowEntity) -> ArrowEntity {
        arrow.add_effect(pumpkin_data::data_component_impl::StatusEffectInstance {
            effect_id: "slowness".into(),
            amplifier: 0,
            duration: 600,
            ambient: false,
            show_particles: true,
            show_icon: true,
        });
        arrow
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.entity.finalize_spawn(world, group_data)
    }
}
