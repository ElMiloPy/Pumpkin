use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_nbt::compound::NbtCompound;

use crate::entity::{
    Entity, EntityBase,
    custom_sound::CustomSound,
    mob::{
        Mob, MobEntity, equipment::RegionalDifficulty, skeleton::SkeletonEntityBase,
        skeleton::stray::StraySkeletonEntity, spawn::SpawnGroupData,
    },
};
use crate::world::World;

pub struct SkeletonEntity {
    entity: Arc<SkeletonEntityBase>,
    pub freezing_time: AtomicI32,
    pub stray_conversion_time: AtomicI32,
}

impl SkeletonEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = SkeletonEntityBase::new(entity);
        let skeleton = Self {
            entity,
            freezing_time: AtomicI32::new(-1),
            stray_conversion_time: AtomicI32::new(-1),
        };
        Arc::new(skeleton)
    }

    pub fn is_converting(&self) -> bool {
        self.stray_conversion_time.load(Ordering::Relaxed) > 0
    }

    pub fn start_freeze_conversion(&self, time: i32) {
        self.stray_conversion_time.store(time, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::skeleton::DATA_STRAY_CONVERSION_ID,
            true,
        );
    }

    pub fn stop_freeze_conversion(&self) {
        self.stray_conversion_time.store(-1, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::skeleton::DATA_STRAY_CONVERSION_ID,
            false,
        );
    }

    fn convert_to_stray(&self) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        let world = entity.world.load();
        let pos = entity.pos.load();
        let block_pos = pos.to_block_pos();

        let packet =
            pumpkin_protocol::java::client::play::CWorldEvent::new(1048, block_pos, 0, false);
        let chunk_pos = entity.chunk_pos.load();
        world.broadcast_to_chunk(chunk_pos, &packet);

        let stray_entity = Entity::new(world.clone(), pos, &EntityType::STRAY);
        stray_entity.yaw.store(entity.yaw.load());
        stray_entity.pitch.store(entity.pitch.load());
        stray_entity.velocity.store(entity.velocity.load());

        let stray = StraySkeletonEntity::new(stray_entity);
        let stray_living = &stray.get_mob_entity().living_entity;
        stray_living.set_health(living.health.load());

        if let Ok(src_eq) = living.entity_equipment.lock()
            && let Ok(mut dst_eq) = stray_living.entity_equipment.lock()
        {
            for slot in &MobEntity::EQUIPMENT_POPULATION_ORDER {
                dst_eq.put(slot, src_eq.get(slot));
            }
        }

        world.spawn_entity(stray);
        entity.remove();
    }
}

impl CustomSound for SkeletonEntity {
    fn death_sound(&self) -> Option<Sound> {
        Some(Sound::EntitySkeletonDeath)
    }

    fn hurt_sound(&self) -> Option<Sound> {
        Some(Sound::EntitySkeletonHurt)
    }

    fn sound_category(&self) -> SoundCategory {
        SoundCategory::Hostile
    }
}

impl Mob for SkeletonEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn as_custom_sound(&self) -> Option<&dyn CustomSound> {
        Some(self)
    }

    fn shoot_sound(&self) -> Option<Sound> {
        Some(Sound::EntitySkeletonShoot)
    }

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        if !entity.is_alive() {
            return;
        }

        if entity.is_in_powder_snow() {
            let conv_time = self.stray_conversion_time.load(Ordering::Relaxed);
            if conv_time > 0 {
                let remaining = conv_time - 1;
                self.stray_conversion_time
                    .store(remaining, Ordering::Relaxed);
                if remaining == 0 {
                    self.convert_to_stray();
                }
            } else {
                let freeze = self.freezing_time.fetch_add(1, Ordering::Relaxed) + 1;
                if freeze >= 140 {
                    self.start_freeze_conversion(300);
                }
            }
        } else {
            self.freezing_time.store(-1, Ordering::Relaxed);
            if self.is_converting() {
                self.stop_freeze_conversion();
            }
        }
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_int("FreezingTime", self.freezing_time.load(Ordering::Relaxed));
        nbt.put_int(
            "StrayConversionTime",
            self.stray_conversion_time.load(Ordering::Relaxed),
        );
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        let freezing = nbt.get_int("FreezingTime").unwrap_or(-1);
        let conversion = nbt.get_int("StrayConversionTime").unwrap_or(-1);
        self.freezing_time.store(freezing, Ordering::Relaxed);
        self.stray_conversion_time
            .store(conversion, Ordering::Relaxed);
        if conversion > 0 {
            self.entity.mob_entity.living_entity.entity.set_synced_data(
                pumpkin_data::tracked_data::skeleton::DATA_STRAY_CONVERSION_ID,
                true,
            );
        }
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.entity.finalize_spawn(world, group_data)
    }

    fn populate_default_equipment_slots(
        &self,
        world: &Arc<World>,
        difficulty: &RegionalDifficulty,
    ) {
        self.entity
            .populate_default_equipment_slots(world, difficulty);
    }

    fn populate_default_equipment_enchantments(&self, difficulty: &RegionalDifficulty) {
        self.entity
            .populate_default_equipment_enchantments(difficulty);
    }
}
