use super::BlockEntity;
use pumpkin_data::block_properties::{BeeNestLikeProperties, HorizontalFacing};
use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::{Block, BlockDirection};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::entity::Entity;
use crate::entity::EntityBase;
use crate::entity::ageable::AgeableMob;
use crate::entity::mob::neutral::NeutralMob;
use crate::entity::passive::bee::BeeEntity;
use crate::entity::player::Player;
use crate::world::World;

pub struct BeehiveBlockEntity {
    pub position: BlockPos,
    pub bees: Mutex<Option<Vec<NbtTag>>>,
    pub flower_pos: Mutex<Option<BlockPos>>,
}

impl BlockEntity for BeehiveBlockEntity {
    fn resource_location(&self) -> &'static str {
        Self::ID
    }

    fn get_position(&self) -> BlockPos {
        self.position
    }

    fn from_nbt(nbt: &pumpkin_nbt::compound::NbtCompound, position: BlockPos) -> Self
    where
        Self: Sized,
    {
        let bees = nbt
            .get_list("bees")
            .or_else(|| nbt.get_list("Bees"))
            .map(<[_]>::to_vec);
        let flower_pos = nbt
            .get_int_array("flower_pos")
            .and_then(|arr| (arr.len() == 3).then(|| BlockPos::new(arr[0], arr[1], arr[2])))
            .or_else(|| {
                nbt.get_compound("flower_pos")
                    .or_else(|| nbt.get_compound("FlowerPos"))
                    .map(|c| {
                        BlockPos::new(
                            c.get_int("X").or_else(|| c.get_int("x")).unwrap_or(0),
                            c.get_int("Y").or_else(|| c.get_int("y")).unwrap_or(0),
                            c.get_int("Z").or_else(|| c.get_int("z")).unwrap_or(0),
                        )
                    })
            });
        Self {
            position,
            bees: Mutex::new(bees),
            flower_pos: Mutex::new(flower_pos),
        }
    }

    fn write_nbt(&self, nbt: &mut NbtCompound) {
        if let Ok(b) = self.bees.lock()
            && let Some(b) = b.as_ref()
        {
            nbt.put_list("bees", b.clone());
        }
        if let Ok(fp) = self.flower_pos.lock()
            && let Some(fp) = fp.as_ref()
        {
            nbt.put(
                "flower_pos",
                pumpkin_nbt::tag::NbtTag::IntArray(vec![fp.0.x, fp.0.y, fp.0.z]),
            );
        }
    }

    fn chunk_data_nbt(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        if let Ok(bees) = self.bees.try_lock()
            && let Some(ref b) = *bees
        {
            nbt.put_list("bees", b.clone());
        }
        if let Ok(flower_pos) = self.flower_pos.try_lock()
            && let Some(ref fp) = *flower_pos
        {
            nbt.put(
                "flower_pos",
                pumpkin_nbt::tag::NbtTag::IntArray(vec![fp.0.x, fp.0.y, fp.0.z]),
            );
        }
        Some(nbt)
    }

    fn collect_item_components(&self, stack: &mut pumpkin_data::item_stack::ItemStack) {
        if let Ok(bees) = self.bees.lock()
            && let Some(bees) = bees.as_ref()
            && !bees.is_empty()
        {
            stack.set_data_component(pumpkin_data::data_component_impl::BeesImpl {
                bees: bees.clone(),
            });
        }
    }

    fn apply_item_components(&self, stack: &pumpkin_data::item_stack::ItemStack) {
        if let Some(bees) =
            stack.get_data_component::<pumpkin_data::data_component_impl::BeesImpl>()
            && !bees.bees.is_empty()
        {
            *self
                .bees
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(bees.bees.clone());
        }
    }

    fn tick(&self, world: &Arc<World>) {
        let block = world.get_block(&self.position);
        if block != &Block::BEEHIVE && block != &Block::BEE_NEST {
            return;
        }

        let is_day = (world.get_time_of_day().rem_euclid(24000)) < 12000;
        let can_exit_normally = is_day && !world.is_raining();

        let mut bees_guard = self
            .bees
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(bees) = bees_guard.as_mut() else {
            return;
        };
        if bees.is_empty() {
            return;
        }

        let mut to_release = Vec::new();
        for (i, tag) in bees.iter_mut().enumerate() {
            if let NbtTag::Compound(compound) = tag {
                let ticks = compound.get_int("ticks_in_hive").unwrap_or(0) + 1;
                compound.put_int("ticks_in_hive", ticks);
                let min_ticks = compound.get_int("min_ticks_before_leaving").unwrap_or(600);
                if ticks >= min_ticks && can_exit_normally {
                    to_release.push(i);
                }
            }
        }

        if to_release.is_empty() {
            return;
        }

        let state_id = world.get_block_state_id(&self.position);
        let props = BeeNestLikeProperties::from_state_id(state_id);
        let facing = match props.facing {
            HorizontalFacing::North => BlockDirection::North,
            HorizontalFacing::South => BlockDirection::South,
            HorizontalFacing::West => BlockDirection::West,
            HorizontalFacing::East => BlockDirection::East,
        };
        let spawn_offset = facing.to_offset();
        let spawn_pos = Vector3::new(
            f64::from(self.position.0.x) + 0.5 + f64::from(spawn_offset.x) * 0.7,
            f64::from(self.position.0.y) + 0.2,
            f64::from(self.position.0.z) + 0.5 + f64::from(spawn_offset.z) * 0.7,
        );

        let mut produced_honey = false;

        for i in to_release.into_iter().rev() {
            let tag = bees.remove(i);
            if let NbtTag::Compound(compound) = tag {
                let entity_data = compound
                    .get_compound("entity_data")
                    .cloned()
                    .unwrap_or_default();
                let had_nectar = entity_data.get_bool("HasNectar").unwrap_or(false);
                if had_nectar {
                    produced_honey = true;
                }
                Self::spawn_bee(world, spawn_pos, entity_data, Some(self.position), None);
            }
        }

        if produced_honey {
            let mut new_props = props;
            if new_props.honey_level < 5 {
                new_props.honey_level += 1;
                world.set_block_state(
                    &self.position,
                    new_props.to_state_id(block),
                    BlockFlags::NOTIFY_ALL,
                );
                world.play_sound(
                    Sound::BlockBeehiveDrip,
                    SoundCategory::Blocks,
                    &self.position.to_f64(),
                );
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl BeehiveBlockEntity {
    pub const ID: &'static str = "minecraft:beehive";

    #[must_use]
    pub const fn new(position: BlockPos) -> Self {
        Self {
            position,
            bees: Mutex::new(None),
            flower_pos: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn get_bee_count(&self) -> usize {
        self.bees
            .lock()
            .map_or(0, |b| b.as_ref().map_or(0, std::vec::Vec::len))
    }

    #[must_use]
    pub fn can_enter_hive(&self) -> bool {
        self.get_bee_count() < 3
    }

    pub fn try_enter_hive(&self, bee: &BeeEntity, world: &Arc<World>) -> bool {
        let mut bees_guard = self
            .bees
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bees = bees_guard.get_or_insert_with(Vec::new);
        if bees.len() >= 3 {
            return false;
        }

        let mut entity_data = NbtCompound::new();
        entity_data.put_string("id", "minecraft:bee".to_string());
        entity_data.put_bool("HasNectar", bee.has_nectar());
        entity_data.put_int("Age", bee.get_age());
        entity_data.put_int(
            "TicksSincePollination",
            bee.ticks_without_nectar.load(Ordering::Relaxed),
        );
        entity_data.put_int(
            "CropsGrownSincePollination",
            bee.crops_grown_since_pollination.load(Ordering::Relaxed),
        );
        entity_data.put_float("Health", bee.mob_entity.living_entity.health.load());
        if let Some(name) = bee.get_entity().custom_name.load().as_ref() {
            entity_data.put_string("CustomName", name.clone().get_text());
        }

        let has_nectar = bee.has_nectar();
        let mut bee_tag = NbtCompound::new();
        bee_tag.put_compound("entity_data", entity_data);
        bee_tag.put_int("ticks_in_hive", 0);
        bee_tag.put_int(
            "min_ticks_before_leaving",
            if has_nectar { 2400 } else { 600 },
        );

        bees.push(NbtTag::Compound(bee_tag));

        world.play_sound(
            Sound::BlockBeehiveEnter,
            SoundCategory::Blocks,
            &self.position.to_f64(),
        );

        bee.get_entity().remove();
        true
    }

    pub fn spawn_bee(
        world: &Arc<World>,
        pos: Vector3<f64>,
        entity_data: NbtCompound,
        hive_pos: Option<BlockPos>,
        anger_target: Option<&Player>,
    ) {
        let entity = Entity::new(world.clone(), pos, &EntityType::BEE);
        let bee = BeeEntity::new(entity);
        bee.cannot_enter_hive_ticks.store(400, Ordering::Relaxed);

        if let Some(hp) = hive_pos {
            *bee.hive_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hp);
        }
        if let Some(age) = entity_data.get_int("Age") {
            bee.set_age(age);
        }
        if let Some(health) = entity_data.get_float("Health") {
            bee.mob_entity.living_entity.health.store(health);
        }
        if let Some(target) = anger_target {
            bee.set_time_to_remain_angry(rand::random_range(400..800));
            bee.set_persistent_anger_target(Some(target.gameprofile.id));
            if let Some(player_arc) = world.get_player_by_uuid(target.gameprofile.id) {
                bee.mob_entity
                    .set_target(Some(player_arc as Arc<dyn EntityBase>));
            }
        }

        world.play_sound(Sound::BlockBeehiveExit, SoundCategory::Blocks, &pos);

        world.spawn_entity(bee);
    }

    pub fn release_all_bees(&self, world: &Arc<World>, anger_target: Option<&Player>) {
        let mut bees_guard = self
            .bees
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(bees) = bees_guard.as_mut() else {
            return;
        };
        if bees.is_empty() {
            return;
        }

        let state_id = world.get_block_state_id(&self.position);
        let props = BeeNestLikeProperties::from_state_id(state_id);
        let facing = match props.facing {
            HorizontalFacing::North => BlockDirection::North,
            HorizontalFacing::South => BlockDirection::South,
            HorizontalFacing::West => BlockDirection::West,
            HorizontalFacing::East => BlockDirection::East,
        };
        let spawn_offset = facing.to_offset();
        let spawn_pos = Vector3::new(
            f64::from(self.position.0.x) + 0.5 + f64::from(spawn_offset.x) * 0.7,
            f64::from(self.position.0.y) + 0.2,
            f64::from(self.position.0.z) + 0.5 + f64::from(spawn_offset.z) * 0.7,
        );

        for tag in bees.drain(..) {
            if let NbtTag::Compound(compound) = tag {
                let entity_data = compound
                    .get_compound("entity_data")
                    .cloned()
                    .unwrap_or_default();
                Self::spawn_bee(world, spawn_pos, entity_data, None, anger_target);
            }
        }
    }

    pub fn anger_bees(&self, target: &Player, world: &Arc<World>) {
        self.release_all_bees(world, Some(target));

        let hive_center = self.position.to_centered_f64();
        let player_arc = world.get_player_by_uuid(target.gameprofile.id);
        for entity in world.entities.load().iter() {
            if entity.get_entity().entity_type != &EntityType::BEE {
                continue;
            }
            if let Some(bee) = entity.cast_any().downcast_ref::<BeeEntity>() {
                let bee_pos = bee.get_entity().pos.load();
                if (bee_pos - hive_center).length_squared() < 16.0 * 16.0 {
                    bee.set_time_to_remain_angry(rand::random_range(400..800));
                    bee.set_persistent_anger_target(Some(target.gameprofile.id));
                    if let Some(ref p) = player_arc {
                        bee.mob_entity
                            .set_target(Some(p.clone() as Arc<dyn EntityBase>));
                    }
                }
            }
        }
    }
}
