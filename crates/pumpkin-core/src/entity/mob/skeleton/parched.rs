use std::sync::Arc;

use crate::entity::{
    Entity,
    mob::{Mob, MobEntity, skeleton::SkeletonEntityBase, spawn::SpawnGroupData},
};
use crate::world::World;

pub struct ParchedSkeletonEntity {
    entity: Arc<SkeletonEntityBase>,
}

impl ParchedSkeletonEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = SkeletonEntityBase::new(entity);
        let parched = Self { entity };
        Arc::new(parched)
    }
}

impl Mob for ParchedSkeletonEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.entity.finalize_spawn(world, group_data)
    }
}
