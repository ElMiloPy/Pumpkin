use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use pumpkin_data::entity::EntityType;
use pumpkin_nbt::compound::NbtCompound;

use crate::entity::mob::equipment::RegionalDifficulty;
use crate::entity::mob::spawn::SpawnGroupData;
use crate::entity::mob::zombie::ZombieEntityBase;
use crate::entity::mob::zombie::drowned::DrownedEntity;
use crate::entity::mob::{Mob, MobEntity};
use crate::entity::{Entity, EntityBase};
use crate::world::World;

pub struct ZombieEntity {
    pub entity: Arc<ZombieEntityBase>,
    pub in_water_time: AtomicI32,
    pub drowned_conversion_time: AtomicI32,
}

impl ZombieEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = ZombieEntityBase::new(entity);
        let zombie = Self {
            entity,
            in_water_time: AtomicI32::new(0),
            drowned_conversion_time: AtomicI32::new(-1),
        };
        Arc::new(zombie)
    }

    #[must_use]
    pub fn with_can_break_doors(entity: Entity, can_break_doors: bool) -> Arc<Self> {
        let entity = ZombieEntityBase::with_can_break_doors(entity, can_break_doors);
        let zombie = Self {
            entity,
            in_water_time: AtomicI32::new(0),
            drowned_conversion_time: AtomicI32::new(-1),
        };
        Arc::new(zombie)
    }

    #[must_use]
    pub fn is_converting(&self) -> bool {
        self.drowned_conversion_time.load(Ordering::Relaxed) > 0
    }

    pub fn start_conversion(&self, time: i32) {
        self.drowned_conversion_time.store(time, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
            true,
        );
    }

    pub fn stop_conversion(&self) {
        self.drowned_conversion_time.store(-1, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
            false,
        );
    }

    fn convert_to_drowned(&self) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        let world = entity.world.load_full();
        let pos = entity.pos.load();
        let block_pos = pos.to_block_pos();

        let packet =
            pumpkin_protocol::java::client::play::CWorldEvent::new(1040, block_pos, 0, false);
        let chunk_pos = entity.chunk_pos.load();
        world.broadcast_to_chunk(chunk_pos, &packet);

        let drowned_entity = Entity::new(world.clone(), pos, &EntityType::DROWNED);
        drowned_entity.yaw.store(entity.yaw.load());
        drowned_entity.pitch.store(entity.pitch.load());
        drowned_entity.velocity.store(entity.velocity.load());

        let drowned = DrownedEntity::new(drowned_entity);
        let drowned_living = &drowned.get_mob_entity().living_entity;
        drowned_living.set_health(living.health.load());
        if self.is_baby() {
            drowned.set_baby(true);
        }

        if let Ok(src_eq) = living.entity_equipment.lock()
            && let Ok(mut dst_eq) = drowned_living.entity_equipment.lock()
        {
            for slot in &MobEntity::EQUIPMENT_POPULATION_ORDER {
                dst_eq.put(slot, src_eq.get(slot));
            }
        }

        if let Some(custom_name) = entity.custom_name.load().as_ref().clone() {
            drowned_living.entity.set_custom_name(custom_name);
        }

        world.spawn_entity(drowned);
        entity.remove();
    }
}

impl Mob for ZombieEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn spawn_as_baby(&self) -> bool {
        self.entity.spawn_as_baby()
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

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        if !entity.is_alive() {
            return;
        }

        if entity.is_submerged_in_water() {
            let conv_time = self.drowned_conversion_time.load(Ordering::Relaxed);
            if conv_time > 0 {
                let remaining = conv_time - 1;
                self.drowned_conversion_time
                    .store(remaining, Ordering::Relaxed);
                if remaining == 0 {
                    self.convert_to_drowned();
                }
            } else {
                let in_water = self.in_water_time.fetch_add(1, Ordering::Relaxed) + 1;
                if in_water >= 600 {
                    self.start_conversion(300);
                }
            }
        } else {
            self.in_water_time.store(-1, Ordering::Relaxed);
            if self.is_converting() {
                self.stop_conversion();
            }
        }
    }

    fn mob_killed_entity(
        &self,
        victim: &dyn EntityBase,
        params: &crate::world::loot::LootContextParameters<'_>,
    ) {
        self.entity.mob_killed_entity(victim, params);
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.entity.mob_write_nbt(nbt);
        nbt.put_int("InWaterTime", self.in_water_time.load(Ordering::Relaxed));
        nbt.put_int(
            "DrownedConversionTime",
            if self.is_converting() {
                self.drowned_conversion_time.load(Ordering::Relaxed)
            } else {
                -1
            },
        );
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.entity.mob_read_nbt(nbt);
        let in_water = nbt.get_int("InWaterTime").unwrap_or(-1);
        let conversion = nbt.get_int("DrownedConversionTime").unwrap_or(-1);
        self.in_water_time.store(in_water, Ordering::Relaxed);
        self.drowned_conversion_time
            .store(conversion, Ordering::Relaxed);
        if conversion > 0 {
            self.entity.mob_entity.living_entity.entity.set_synced_data(
                pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
                true,
            );
        }
    }
}

impl ZombieEntity {
    #[must_use]
    pub fn can_break_doors(&self) -> bool {
        self.entity
            .can_break_doors
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_can_break_doors(&self, can_break: bool) {
        self.entity
            .can_break_doors
            .store(can_break, std::sync::atomic::Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.entity.is_baby()
    }

    pub fn set_baby(&self, baby: bool) {
        self.entity.set_baby(baby);
    }
}
