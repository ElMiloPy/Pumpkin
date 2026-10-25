use pumpkin_data::block_properties::{
    DoubleBlockHalf, SmallDripleafLikeProperties, TallSeagrassLikeProperties,
};
use pumpkin_data::{Block, BlockState};
use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use crate::generation::proto_chunk::GenerationCache;
use crate::{
    generation::block_state_provider::BlockStateProvider,
    world::{BlockAccessor, WorldPortalExt},
};

pub struct SimpleBlockFeature {
    pub to_place: BlockStateProvider,
    pub schedule_tick: Option<bool>,
}

fn is_tall_plant(block: &Block) -> bool {
    block.id == Block::SUNFLOWER.id
        || block.id == Block::LILAC.id
        || block.id == Block::PEONY.id
        || block.id == Block::ROSE_BUSH.id
        || block.id == Block::TALL_GRASS.id
        || block.id == Block::LARGE_FERN.id
        || block.id == Block::PITCHER_PLANT.id
}

impl SimpleBlockFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        let state = self.to_place.get(random, pos, chunk, block_registry);
        let block = Block::from_state_id(state.id);
        let block_accessor: &dyn BlockAccessor = chunk;
        if !block_registry.can_place_at(block, state, block_accessor, &pos) {
            return false;
        }

        if is_tall_plant(block) {
            let above = pos.up();
            if !chunk.is_air(&above.0) {
                return false;
            }
            let mut lower_props = TallSeagrassLikeProperties::from_state_id(state.id);
            lower_props.half = DoubleBlockHalf::Lower;
            let lower_state = BlockState::from_id(lower_props.to_state_id(block));

            let mut upper_props = TallSeagrassLikeProperties::from_state_id(state.id);
            upper_props.half = DoubleBlockHalf::Upper;
            let upper_state = BlockState::from_id(upper_props.to_state_id(block));

            chunk.set_block_state(&pos.0, lower_state);
            chunk.set_block_state(&above.0, upper_state);
            return true;
        }

        if block == &Block::SMALL_DRIPLEAF {
            let above = pos.up();
            if !chunk.is_air(&above.0) {
                return false;
            }
            let mut lower_props = SmallDripleafLikeProperties::from_state_id(state.id);
            lower_props.half = DoubleBlockHalf::Lower;
            let lower_state = BlockState::from_id(lower_props.to_state_id(block));

            let mut upper_props = SmallDripleafLikeProperties::from_state_id(state.id);
            upper_props.half = DoubleBlockHalf::Upper;
            let upper_state = BlockState::from_id(upper_props.to_state_id(block));

            chunk.set_block_state(&pos.0, lower_state);
            chunk.set_block_state(&above.0, upper_state);
            return true;
        }

        chunk.set_block_state(&pos.0, state);
        true
    }
}
