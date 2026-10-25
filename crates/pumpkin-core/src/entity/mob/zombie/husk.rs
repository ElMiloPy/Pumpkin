use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::potion::Effect;
use pumpkin_nbt::compound::NbtCompound;

use crate::entity::mob::equipment::RegionalDifficulty;
use crate::entity::mob::spawn::SpawnGroupData;
use crate::entity::mob::zombie::ZombieEntityBase;
use crate::entity::mob::zombie::zombie::ZombieEntity;
use crate::entity::mob::{Mob, MobEntity};
use crate::entity::{Entity, EntityBase};
use crate::world::World;

pub struct HuskEntity {
    pub entity: Arc<ZombieEntityBase>,
    pub in_water_time: AtomicI32,
    pub zombie_conversion_time: AtomicI32,
}

impl HuskEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = ZombieEntityBase::new(entity);
        let zombie = Self {
            entity,
            in_water_time: AtomicI32::new(0),
            zombie_conversion_time: AtomicI32::new(-1),
        };
        Arc::new(zombie)
    }

    #[must_use]
    pub fn with_can_break_doors(entity: Entity, can_break_doors: bool) -> Arc<Self> {
        let entity = ZombieEntityBase::with_can_break_doors(entity, can_break_doors);
        let zombie = Self {
            entity,
            in_water_time: AtomicI32::new(0),
            zombie_conversion_time: AtomicI32::new(-1),
        };
        Arc::new(zombie)
    }

    #[must_use]
    pub fn is_converting(&self) -> bool {
        self.zombie_conversion_time.load(Ordering::Relaxed) > 0
    }

    pub fn start_conversion(&self, time: i32) {
        self.zombie_conversion_time.store(time, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
            true,
        );
    }

    pub fn stop_conversion(&self) {
        self.zombie_conversion_time.store(-1, Ordering::Relaxed);
        self.entity.mob_entity.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
            false,
        );
    }

    fn convert_to_zombie(&self) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        let world = entity.world.load_full();
        let pos = entity.pos.load();
        let block_pos = pos.to_block_pos();

        let packet =
            pumpkin_protocol::java::client::play::CWorldEvent::new(1041, block_pos, 0, false);
        let chunk_pos = entity.chunk_pos.load();
        world.broadcast_to_chunk(chunk_pos, &packet);

        let zombie_entity = Entity::new(world.clone(), pos, &EntityType::ZOMBIE);
        zombie_entity.yaw.store(entity.yaw.load());
        zombie_entity.pitch.store(entity.pitch.load());
        zombie_entity.velocity.store(entity.velocity.load());

        let zombie = ZombieEntity::new(zombie_entity);
        let zombie_living = &zombie.get_mob_entity().living_entity;
        zombie_living.set_health(living.health.load());
        if self.is_baby() {
            zombie.set_baby(true);
        }

        if let Ok(src_eq) = living.entity_equipment.lock()
            && let Ok(mut dst_eq) = zombie_living.entity_equipment.lock()
        {
            for slot in &MobEntity::EQUIPMENT_POPULATION_ORDER {
                dst_eq.put(slot, src_eq.get(slot));
            }
        }

        if let Some(custom_name) = entity.custom_name.load().as_ref().clone() {
            zombie_living.entity.set_custom_name(custom_name);
        }

        world.spawn_entity(zombie);
        entity.remove();
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.entity.is_baby()
    }

    pub fn set_baby(&self, baby: bool) {
        self.entity.set_baby(baby);
    }
}

impl Mob for HuskEntity {
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

    fn on_attack(&self, target: &dyn EntityBase) {
        let is_unarmed = self
            .get_mob_entity()
            .living_entity
            .entity_equipment
            .try_lock()
            .map_or(true, |eq| eq.get(&EquipmentSlot::MAIN_HAND).is_empty());

        if is_unarmed && let Some(living) = target.get_living_entity() {
            let world = self.entity.mob_entity.living_entity.entity.world.load();
            let difficulty = match world.level_info.load().difficulty {
                pumpkin_util::Difficulty::Normal => 2,
                pumpkin_util::Difficulty::Hard => 3,
                _ => 1,
            };
            living.add_effect(Effect {
                effect_type: &StatusEffect::HUNGER,
                duration: 140 * difficulty,
                amplifier: 0,
                ambient: false,
                show_particles: true,
                show_icon: true,
                blend: false,
            });
        }
    }

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        let living = &self.entity.mob_entity.living_entity;
        let entity = &living.entity;
        if !entity.is_alive() {
            return;
        }

        if entity.is_submerged_in_water() {
            let conv_time = self.zombie_conversion_time.load(Ordering::Relaxed);
            if conv_time > 0 {
                let remaining = conv_time - 1;
                self.zombie_conversion_time
                    .store(remaining, Ordering::Relaxed);
                if remaining == 0 {
                    self.convert_to_zombie();
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
                self.zombie_conversion_time.load(Ordering::Relaxed)
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
        self.zombie_conversion_time
            .store(conversion, Ordering::Relaxed);
        if conversion > 0 {
            self.entity.mob_entity.living_entity.entity.set_synced_data(
                pumpkin_data::tracked_data::zombie::DATA_DROWNED_CONVERSION_ID,
                true,
            );
        }
    }
}
