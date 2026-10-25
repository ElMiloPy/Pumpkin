use pumpkin_data::{
    Block, BlockDirection, BlockState,
    block_properties::{BeehiveProperties, HorizontalFacing},
};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::proto_chunk::GenerationCache;

pub struct BeehiveTreeDecorator {
    pub probability: f32,
}

impl BeehiveTreeDecorator {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        log_positions: &[BlockPos],
        foliage_positions: &[BlockPos],
        decorations: &mut Vec<BlockPos>,
    ) {
        if log_positions.is_empty() || random.next_f32() >= self.probability {
            return;
        }

        let worldgen_facing = BlockDirection::South;
        let spawn_directions = [
            BlockDirection::East,
            BlockDirection::South,
            BlockDirection::West,
        ];

        let hive_y = foliage_positions.first().map_or_else(
            || {
                let last_log_y = log_positions.last().map_or(0, |p| p.0.y);
                (log_positions[0].0.y + 1 + random.next_bounded_i32(3)).min(last_log_y)
            },
            |first_leaf| (first_leaf.0.y - 1).max(log_positions[0].0.y + 1),
        );

        let mut hive_placements: Vec<BlockPos> = Vec::new();
        for log_pos in log_positions.iter().filter(|p| p.0.y == hive_y) {
            for dir in spawn_directions {
                hive_placements.push(log_pos.offset(dir.to_offset()));
            }
        }

        if hive_placements.is_empty() {
            return;
        }

        for i in (1..hive_placements.len()).rev() {
            let j = random.next_bounded_i32((i + 1) as i32) as usize;
            hive_placements.swap(i, j);
        }

        for pos in hive_placements {
            let in_front = pos.offset(worldgen_facing.to_offset());
            if chunk.is_air(&pos.0) && chunk.is_air(&in_front.0) {
                let mut props = BeehiveProperties::default(&Block::BEE_NEST);
                props.facing = HorizontalFacing::South;
                let state_id = props.to_state_id(&Block::BEE_NEST);
                chunk.set_block_state(&pos.0, BlockState::from_id(state_id));

                let mut be_nbt = NbtCompound::new();
                be_nbt.put_string("id", "minecraft:beehive".to_string());
                be_nbt.put_int("x", pos.0.x);
                be_nbt.put_int("y", pos.0.y);
                be_nbt.put_int("z", pos.0.z);

                let mut bees_list = Vec::new();
                for _ in 0..3 {
                    let mut bee_tag = NbtCompound::new();
                    let mut entity_data = NbtCompound::new();
                    entity_data.put_string("id", "minecraft:bee".to_string());
                    bee_tag.put_compound("entity_data", entity_data);
                    bee_tag.put_int("ticks_in_hive", random.next_bounded_i32(599));
                    bee_tag.put_int("min_ticks_before_leaving", 600);
                    bees_list.push(NbtTag::Compound(bee_tag));
                }
                be_nbt.put_list("bees", bees_list);
                chunk.add_block_entity(&pos.0, be_nbt);

                decorations.push(pos);
                break;
            }
        }
    }
}
