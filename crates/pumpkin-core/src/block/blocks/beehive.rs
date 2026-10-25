use crate::block::entities::beehive::BeehiveBlockEntity;
use crate::block::registry::BlockActionResult;
use crate::block::{
    BlockBehaviour, BlockMetadata, BrokenArgs, GetComparatorOutputArgs, UseWithItemArgs,
};
use crate::entity::Entity;
use crate::entity::item::ItemEntity;
use pumpkin_data::block_properties::{BeeNestLikeProperties, CampfireLikeProperties};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::{Block, BlockId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;
use std::sync::Arc;

pub struct BeehiveBlock;

impl BeehiveBlock {
    fn is_smoked(world: &crate::world::World, pos: &BlockPos) -> bool {
        for i in 1..=5 {
            let check_pos = pos.down_height(i);
            let (block, state_id) = world.get_block_and_state_id(&check_pos);
            if (block == &Block::CAMPFIRE || block == &Block::SOUL_CAMPFIRE)
                && CampfireLikeProperties::from_state_id(state_id).lit
            {
                return true;
            }
        }
        false
    }
}

impl BlockMetadata for BeehiveBlock {
    fn ids() -> Box<[BlockId]> {
        [BlockId::BEEHIVE, BlockId::BEE_NEST].into()
    }
}

impl BlockBehaviour for BeehiveBlock {
    fn get_comparator_output(&self, args: GetComparatorOutputArgs<'_>) -> Option<u8> {
        let state_id = args.world.get_block_state_id(args.position);
        let props = BeeNestLikeProperties::from_state_id(state_id);
        Some(props.honey_level)
    }

    fn use_with_item(&self, args: UseWithItemArgs<'_>) -> BlockActionResult {
        let state_id = args.world.get_block_state_id(args.position);
        let mut props = BeeNestLikeProperties::from_state_id(state_id);
        if props.honey_level < 5 {
            return BlockActionResult::Pass;
        }

        let item = args.item_stack.item;
        if item == &Item::SHEARS {
            args.world.play_sound(
                Sound::BlockBeehiveShear,
                SoundCategory::Blocks,
                &args.position.to_f64(),
            );
            let entity = Entity::new(
                args.world.clone(),
                args.position.to_centered_f64(),
                &EntityType::ITEM,
            );
            let item_entity =
                Arc::new(ItemEntity::new(entity, ItemStack::new(3, &Item::HONEYCOMB)));
            args.world.spawn_entity(item_entity);
            args.player.damage_item_in_slot(args.equipment_slot, 1);

            props.honey_level = 0;
            args.world.set_block_state(
                args.position,
                props.to_state_id(args.block),
                BlockFlags::NOTIFY_ALL,
            );

            if !Self::is_smoked(args.world, args.position)
                && let Some(block_entity) = args.world.get_block_entity(args.position)
                && let Some(beehive) = block_entity.as_any().downcast_ref::<BeehiveBlockEntity>()
            {
                beehive.anger_bees(args.player, args.world);
            }
            return BlockActionResult::SuccessServer;
        } else if item == &Item::GLASS_BOTTLE {
            args.item_stack
                .decrement_unless_creative(args.player.gamemode.load(), 1);
            args.world.play_sound(
                Sound::ItemBottleFill,
                SoundCategory::Blocks,
                &args.position.to_f64(),
            );
            let entity = Entity::new(
                args.world.clone(),
                args.position.to_centered_f64(),
                &EntityType::ITEM,
            );
            let item_entity = Arc::new(ItemEntity::new(
                entity,
                ItemStack::new(1, &Item::HONEY_BOTTLE),
            ));
            args.world.spawn_entity(item_entity);

            props.honey_level = 0;
            args.world.set_block_state(
                args.position,
                props.to_state_id(args.block),
                BlockFlags::NOTIFY_ALL,
            );

            if !Self::is_smoked(args.world, args.position)
                && let Some(block_entity) = args.world.get_block_entity(args.position)
                && let Some(beehive) = block_entity.as_any().downcast_ref::<BeehiveBlockEntity>()
            {
                beehive.anger_bees(args.player, args.world);
            }
            return BlockActionResult::SuccessServer;
        }

        BlockActionResult::Pass
    }

    fn broken(&self, args: BrokenArgs<'_>) {
        if let Some(block_entity) = args.world.get_block_entity(args.position)
            && let Some(beehive) = block_entity.as_any().downcast_ref::<BeehiveBlockEntity>()
        {
            if Self::is_smoked(args.world, args.position) {
                beehive.release_all_bees(args.world, None);
            } else {
                beehive.anger_bees(args.player, args.world);
            }
        }
    }
}
