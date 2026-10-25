use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use crossbeam::atomic::AtomicCell;
use pumpkin_data::Block;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::{EntityStatus, EntityType};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::tracked_data;
use pumpkin_data::world::WorldEvent;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use uuid::Uuid;

use crate::entity::mob::spawn::SpawnGroupData;
use crate::entity::mob::zombie::ZombieEntityBase;
use crate::entity::mob::{Mob, MobEntity};
use crate::entity::passive::villager::VillagerEntity;
use crate::entity::passive::villager::data::{GossipType, VillagerData};
use crate::entity::player::Player;
use crate::entity::player::advancement::trigger::AdvancementTrigger;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Blocks that make a zombie villager's cure tick down faster: iron bars and beds.
fn speeds_up_conversion(block: &Block) -> bool {
    block == &Block::IRON_BARS || block.has_tag(&tag::Block::MINECRAFT_BEDS)
}

pub struct ZombieVillagerEntity {
    pub mob_entity: Arc<ZombieEntityBase>,
    villager_data: std::sync::Mutex<VillagerData>,
    /// `Offers` and `Gossips` kept as the NBT vanilla stores them, so a cure can
    /// hand them straight back to the villager it converts into.
    villager_nbt: std::sync::Mutex<NbtCompound>,
    villager_xp: AtomicI32,
    /// Ticks left until the cure completes; `0` means the mob is not converting.
    conversion_time: AtomicI32,
    conversion_starter: AtomicCell<Option<Uuid>>,
}

impl ZombieVillagerEntity {
    const VILLAGER_CONVERSION_WAIT_MIN: i32 = 3600;
    const VILLAGER_CONVERSION_WAIT_MAX: i32 = 6000;
    const MAX_SPECIAL_BLOCKS_COUNT: i32 = 14;
    const SPECIAL_BLOCK_RADIUS_X: i32 = 4;
    const SPECIAL_BLOCK_RADIUS_Y: i32 = 4;
    const SPECIAL_BLOCK_RADIUS_Z: i32 = 4;
    /// Nausea the cured villager wakes up with.
    const NAUSEA_DURATION: i32 = 200;

    pub fn new(entity: Entity) -> Arc<Self> {
        Self::with_can_break_doors(entity, false)
    }

    #[must_use]
    pub fn with_can_break_doors(entity: Entity, can_break_doors: bool) -> Arc<Self> {
        let mob_entity = ZombieEntityBase::with_can_break_doors(entity, can_break_doors);
        Arc::new(Self {
            mob_entity,
            villager_data: std::sync::Mutex::new(VillagerData::new(
                pumpkin_data::villager::VillagerType::Plains,
                // Vanilla's `initializeZombieVillagerData` rolls a random profession
                // so a cured villager has trades. NBT-loaded data overwrites this later.
                pumpkin_data::villager::VillagerProfession::from_i32(rand::random_range(0..15))
                    .unwrap_or(pumpkin_data::villager::VillagerProfession::None),
                1,
            )),
            villager_nbt: std::sync::Mutex::new(NbtCompound::new()),
            villager_xp: AtomicI32::new(0),
            conversion_time: AtomicI32::new(0),
            conversion_starter: AtomicCell::new(None),
        })
    }

    #[must_use]
    pub fn get_villager_data(&self) -> VillagerData {
        *self
            .villager_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_villager_data(&self, data: VillagerData) {
        *self
            .villager_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = data;
        self.get_entity()
            .set_synced_data(tracked_data::zombie_villager::VILLAGER_DATA, data);
    }

    #[must_use]
    pub fn is_converting(&self) -> bool {
        self.conversion_time.load(Ordering::Relaxed) > 0
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.mob_entity.is_baby()
    }

    pub fn set_baby(&self, baby: bool) {
        self.mob_entity.set_baby(baby);
        self.get_entity()
            .set_synced_data(tracked_data::zombie_villager::DATA_BABY_ID, baby);
    }

    /// Vanilla `Zombie.convertVillagerToZombieVillager`: replaces the villager with
    /// a matching zombie villager carrying over profession, trades, gossips, and XP.
    pub fn convert_from_villager(
        world: &Arc<World>,
        villager: &VillagerEntity,
        is_silent: bool,
    ) -> Arc<Self> {
        let v_entity = villager.get_entity();
        let zombie_villager = Self::new(Entity::new(
            world.clone(),
            v_entity.pos.load(),
            &EntityType::ZOMBIE_VILLAGER,
        ));
        let zv_entity = zombie_villager.get_entity();
        zv_entity.set_rotation(v_entity.yaw.load(), v_entity.pitch.load());
        zv_entity.head_yaw.store(v_entity.head_yaw.load());
        zv_entity.velocity.store(v_entity.velocity.load());
        zv_entity.invulnerable.store(
            v_entity.invulnerable.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        if v_entity.age.load(Ordering::Relaxed) < 0 {
            zombie_villager.set_baby(true);
            zv_entity
                .age
                .store(v_entity.age.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        if let Some(custom_name) = &**v_entity.custom_name.load() {
            zv_entity.set_custom_name(custom_name.clone());
            zv_entity.set_custom_name_visible(v_entity.custom_name_visible.load(Ordering::Relaxed));
        }
        zombie_villager
            .mob_entity
            .mob_entity
            .set_no_ai(villager.mob_entity.is_no_ai());
        zombie_villager
            .mob_entity
            .mob_entity
            .persistence_required
            .store(
                villager
                    .mob_entity
                    .persistence_required
                    .load(Ordering::Relaxed),
                Ordering::Relaxed,
            );

        // Copy equipment & drop chances if any
        let src_equipment = villager
            .mob_entity
            .living_entity
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut dst_equipment = zombie_villager
            .mob_entity
            .mob_entity
            .living_entity
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (*dst_equipment).clone_from(&src_equipment);
        drop(src_equipment);
        drop(dst_equipment);

        let src_chances = villager
            .mob_entity
            .living_entity
            .equipment_drop_chances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut dst_chances = zombie_villager
            .mob_entity
            .mob_entity
            .living_entity
            .equipment_drop_chances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (*dst_chances).clone_from(&src_chances);
        drop(src_chances);
        drop(dst_chances);

        // Copy villager data, offers, gossips, and trading XP
        let villager_data = villager.get_villager_data();
        let villager_xp = villager.xp.load(Ordering::Relaxed);
        let mut villager_nbt = NbtCompound::new();
        villager.mob_write_nbt(&mut villager_nbt);

        let mut carried = NbtCompound::new();
        if let Some(offers) = villager_nbt.get_compound("Offers") {
            carried.put_compound("Offers", offers.clone());
        }
        if let Some(gossips) = villager_nbt.get_list("Gossips") {
            carried.put_list("Gossips", gossips.to_vec());
        }
        *zombie_villager
            .villager_nbt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = carried;

        zombie_villager.set_villager_data(villager_data);
        zombie_villager
            .villager_xp
            .store(villager_xp, Ordering::Relaxed);

        world.spawn_entity(zombie_villager.clone());

        if !is_silent {
            world.sync_world_event(
                WorldEvent::SoundZombieInfected,
                v_entity.block_pos.load(),
                0,
            );
        }

        zombie_villager
    }

    /// Vanilla `startConverting`: begin the cure, swapping Weakness for Strength
    /// and telling clients to play the cure sound.
    pub fn start_converting(&self, starter: Option<Uuid>, ticks: i32) {
        let living = &self.mob_entity.mob_entity.living_entity;
        let entity = &living.entity;

        self.conversion_starter.store(starter);
        self.conversion_time.store(ticks, Ordering::Relaxed);
        entity.set_synced_data(tracked_data::zombie_villager::DATA_CONVERTING_ID, true);
        living.remove_effect(&StatusEffect::WEAKNESS);
        living.add_effect(Effect {
            effect_type: &StatusEffect::STRENGTH,
            duration: ticks,
            // Vanilla computes `min(difficulty - 1, 0)`, which is 0 on every
            // difficulty a zombie villager can be cured on.
            amplifier: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            blend: false,
        });
        entity
            .world
            .load()
            .send_entity_status(entity, EntityStatus::ZombieConverting, None);
    }

    /// Vanilla `getConversionProgress`: iron bars and beds nearby speed the cure up.
    #[expect(clippy::cast_possible_truncation)]
    fn conversion_progress(&self) -> i32 {
        let mut progress = 1;
        if rand::random::<f32>() >= 0.01 {
            return progress;
        }

        let entity = self.get_entity();
        let world = entity.world.load();
        let pos = entity.pos.load();
        let (x, y, z) = (pos.x as i32, pos.y as i32, pos.z as i32);
        let mut special_blocks = 0;

        for block_x in x - Self::SPECIAL_BLOCK_RADIUS_X..x + Self::SPECIAL_BLOCK_RADIUS_X {
            for block_y in y - Self::SPECIAL_BLOCK_RADIUS_Y..y + Self::SPECIAL_BLOCK_RADIUS_Y {
                for block_z in z - Self::SPECIAL_BLOCK_RADIUS_Z..z + Self::SPECIAL_BLOCK_RADIUS_Z {
                    if special_blocks >= Self::MAX_SPECIAL_BLOCKS_COUNT {
                        return progress;
                    }
                    let block = world.get_block(&BlockPos::new(block_x, block_y, block_z));
                    if speeds_up_conversion(block) {
                        if rand::random::<f32>() < 0.3 {
                            progress += 1;
                        }
                        special_blocks += 1;
                    }
                }
            }
        }

        progress
    }

    /// Vanilla `finishConversion`: replace the zombie villager with the villager
    /// it remembers being.
    fn finish_conversion(&self) {
        let entity = self.get_entity();
        let world = entity.world.load();

        let villager = VillagerEntity::new(Entity::new(
            world.clone(),
            entity.pos.load(),
            &EntityType::VILLAGER,
        ));
        let villager_entity = villager.get_entity();
        villager_entity.set_rotation(entity.yaw.load(), entity.pitch.load());
        villager_entity.head_yaw.store(entity.head_yaw.load());
        villager_entity.velocity.store(entity.velocity.load());
        villager_entity.invulnerable.store(
            entity.invulnerable.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        if self.is_baby() {
            villager_entity
                .age
                .store(entity.age.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        if let Some(custom_name) = &**entity.custom_name.load() {
            villager_entity.set_custom_name(custom_name.clone());
            villager_entity
                .set_custom_name_visible(entity.custom_name_visible.load(Ordering::Relaxed));
        }
        villager
            .mob_entity
            .set_no_ai(self.mob_entity.mob_entity.is_no_ai());
        villager.mob_entity.persistence_required.store(
            self.mob_entity
                .mob_entity
                .persistence_required
                .load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

        // Drop preserved equipment held by the zombie villager.
        let mut equipment = self
            .mob_entity
            .mob_entity
            .living_entity
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let drop_chances = self
            .mob_entity
            .mob_entity
            .living_entity
            .equipment_drop_chances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let entity_pos = entity.block_pos.load();
        for (slot, stack) in equipment.equipment.drain() {
            if !stack.is_empty() {
                let chance = drop_chances
                    .get(&slot)
                    .copied()
                    .unwrap_or(crate::entity::mob::equipment::DEFAULT_EQUIPMENT_DROP_CHANCE);
                if chance > 1.0 {
                    world.drop_stack(&entity_pos, stack);
                }
            }
        }
        drop(equipment);
        drop(drop_chances);

        // Hand the villager back its profession, trades, gossips and trading xp.
        let data = self.get_villager_data();
        let mut nbt = self
            .villager_nbt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        nbt.put_int("Xp", self.villager_xp.load(Ordering::Relaxed));
        villager.mob_read_nbt(&nbt);
        villager.set_villager_data(data);

        // If the cured villager has a profession but no trade offers (e.g. naturally spawned zombie villager),
        // generate initial trades up to its current level.
        let has_offers = !villager
            .offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty();
        let profession = data.profession_enum();
        if !has_offers
            && profession != pumpkin_data::villager::VillagerProfession::None
            && profession != pumpkin_data::villager::VillagerProfession::Nitwit
        {
            let max_level = data.level.0.clamp(1, 5);
            for lvl in 1..=max_level {
                villager.add_trades(profession, lvl);
            }
        }

        if let Some(starter) = self.conversion_starter.load() {
            // Vanilla's `ZOMBIE_VILLAGER_CURED` reputation event.
            let mut gossips = villager
                .gossips
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let entries = gossips.entry(starter).or_default();
            for (gossip_type, amount) in [
                (GossipType::MajorPositive, 20),
                (GossipType::MinorPositive, 25),
            ] {
                let value = entries.entry(gossip_type).or_default();
                *value = (*value + amount).min(gossip_type.max_value());
            }
            drop(gossips);

            if let Some(player) = world.get_player_by_uuid(starter) {
                player.trigger_advancement(AdvancementTrigger::CuredZombieVillager);
            }
        }

        villager.mob_entity.living_entity.add_effect(Effect {
            effect_type: &StatusEffect::NAUSEA,
            duration: Self::NAUSEA_DURATION,
            amplifier: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            blend: false,
        });

        world.spawn_entity(villager);
        entity.remove();

        if !entity.is_silent() {
            world.sync_world_event(WorldEvent::SoundZombieConverted, entity.block_pos.load(), 0);
        }
    }
}

impl Mob for ZombieVillagerEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity.mob_entity
    }

    fn spawn_as_baby(&self) -> bool {
        self.mob_entity.spawn_as_baby()
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.mob_entity.finalize_spawn(world, group_data)
    }

    fn remove_when_far_away(&self, _distance_sq: f64) -> bool {
        !self.is_converting() && self.villager_xp.load(Ordering::Relaxed) == 0
    }

    fn mob_metadata(&self) -> Option<crate::entity::mob::MobMetadata> {
        let mut meta = crate::entity::mob::MobMetadata::new()
            .zombie_villager_data(
                self.get_villager_data(),
                self.villager_xp.load(Ordering::Relaxed),
            )
            .converting(self.is_converting());
        if self.is_baby() {
            meta = meta.baby(true);
        }
        Some(meta)
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        self.mob_interact_with_hand(player, item_stack, Hand::Right)
            .unwrap_or(false)
    }

    fn mob_interact_with_hand(
        &self,
        player: &Arc<Player>,
        item_stack: &mut ItemStack,
        _hand: Hand,
    ) -> Option<bool> {
        if item_stack.get_item() == &Item::GOLDEN_APPLE {
            if !self.is_converting()
                && self
                    .mob_entity
                    .mob_entity
                    .living_entity
                    .has_effect(&StatusEffect::WEAKNESS)
            {
                item_stack.decrement_unless_creative(player.gamemode.load(), 1);
                self.start_converting(
                    Some(player.get_entity().entity_uuid),
                    rand::random_range(
                        Self::VILLAGER_CONVERSION_WAIT_MIN..=Self::VILLAGER_CONVERSION_WAIT_MAX,
                    ),
                );
            }
            return Some(true);
        }
        if self.mob_entity.mob_interact(player, item_stack) {
            return Some(true);
        }
        None
    }

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        if !self.get_entity().is_alive() || !self.is_converting() {
            return;
        }
        let progress = self.conversion_progress();
        if self.conversion_time.fetch_sub(progress, Ordering::Relaxed) - progress <= 0 {
            self.conversion_time.store(0, Ordering::Relaxed);
            self.finish_conversion();
        }
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.mob_entity.mob_write_nbt(nbt);

        let data = self.get_villager_data();
        let mut villager_data_nbt = NbtCompound::new();
        villager_data_nbt.put_int("Type", data.r#type.0);
        villager_data_nbt.put_int("Profession", data.profession.0);
        villager_data_nbt.put_int("Level", data.level.0);
        nbt.put_compound("VillagerData", villager_data_nbt);
        nbt.put_int("Xp", self.villager_xp.load(Ordering::Relaxed));

        {
            let carried = self
                .villager_nbt
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(offers) = carried.get_compound("Offers") {
                nbt.put_compound("Offers", offers.clone());
            }
            if let Some(gossips) = carried.get_list("Gossips") {
                nbt.put_list("Gossips", gossips.to_vec());
            }
        }

        let conversion_time = self.conversion_time.load(Ordering::Relaxed);
        nbt.put_int(
            "ConversionTime",
            if conversion_time > 0 {
                conversion_time
            } else {
                -1
            },
        );
        if let Some(starter) = self.conversion_starter.load() {
            nbt.put_uuid("ConversionPlayer", starter);
        }
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.mob_entity.mob_read_nbt(nbt);

        if let Some(villager_data_nbt) = nbt.get_compound("VillagerData") {
            let mut data = self.get_villager_data();
            if let Some(r#type) = villager_data_nbt.get_int("Type") {
                data.r#type = VarInt(r#type);
            }
            if let Some(profession) = villager_data_nbt.get_int("Profession") {
                data.profession = VarInt(profession);
            }
            if let Some(level) = villager_data_nbt.get_int("Level") {
                data.level = VarInt(level);
            }
            self.set_villager_data(data);
        }

        if let Some(xp) = nbt.get_int("Xp") {
            self.villager_xp.store(xp, Ordering::Relaxed);
        }

        let mut carried = NbtCompound::new();
        if let Some(offers) = nbt.get_compound("Offers") {
            carried.put_compound("Offers", offers.clone());
        }
        if let Some(gossips) = nbt.get_list("Gossips") {
            carried.put_list("Gossips", gossips.to_vec());
        }
        *self
            .villager_nbt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = carried;

        if let Some(conversion_time) = nbt.get_int("ConversionTime")
            && conversion_time > 0
        {
            self.start_converting(nbt.get_uuid("ConversionPlayer"), conversion_time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::data_component_impl::EquipmentSlot;
    use pumpkin_util::math::vector3::Vector3;

    #[test]
    fn only_iron_bars_and_beds_speed_up_the_cure() {
        assert!(speeds_up_conversion(&Block::IRON_BARS));
        assert!(speeds_up_conversion(&Block::RED_BED));
        assert!(speeds_up_conversion(&Block::WHITE_BED));
        assert!(!speeds_up_conversion(&Block::COPPER_BARS));
        assert!(!speeds_up_conversion(&Block::STONE));
        assert!(!speeds_up_conversion(&Block::RED_WOOL));
    }

    #[tokio::test]
    async fn start_converting_state_changes() -> Result<(), Box<dyn std::error::Error>> {
        let folder = tempfile::tempdir()?;
        let world = crate::entity::ai::test_support::world(&folder);
        let zv = ZombieVillagerEntity::new(Entity::new(
            world,
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::ZOMBIE_VILLAGER,
        ));
        assert!(!zv.is_converting());

        zv.mob_entity.mob_entity.living_entity.add_effect(Effect {
            effect_type: &StatusEffect::WEAKNESS,
            duration: 1000,
            amplifier: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            blend: false,
        });
        assert!(
            zv.mob_entity
                .mob_entity
                .living_entity
                .has_effect(&StatusEffect::WEAKNESS)
        );

        zv.start_converting(None, 4000);
        assert!(zv.is_converting());
        assert!(
            !zv.mob_entity
                .mob_entity
                .living_entity
                .has_effect(&StatusEffect::WEAKNESS)
        );
        assert!(
            zv.mob_entity
                .mob_entity
                .living_entity
                .has_effect(&StatusEffect::STRENGTH)
        );

        Ok(())
    }

    #[tokio::test]
    async fn finish_conversion_drops_preserved_equipment() -> Result<(), Box<dyn std::error::Error>>
    {
        let folder = tempfile::tempdir()?;
        let world = crate::entity::ai::test_support::world(&folder);
        let zv = ZombieVillagerEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::ZOMBIE_VILLAGER,
        ));
        zv.mob_entity.mob_entity.set_item_slot_and_drop_when_killed(
            &EquipmentSlot::MAIN_HAND,
            ItemStack::new(1, &Item::DIAMOND_SWORD),
        );
        zv.finish_conversion();
        let entities = world.entities.load_full();
        assert!(
            entities
                .iter()
                .any(|e| *e.get_entity().entity_type == EntityType::ITEM),
            "preserved item was not dropped into the world"
        );
        Ok(())
    }
}
