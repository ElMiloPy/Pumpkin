#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    pub fn handle_pick_item_from_block(
        &self,
        player: &Arc<Player>,
        pick_item: &SPickItemFromBlock,
    ) {
        if !player.can_interact_with_block_at(&pick_item.pos, 1.0) {
            return;
        }

        let world = player.world();
        let block = world.get_block(&pick_item.pos);

        if block.item_id == 0 {
            // Invalid block id (blocks such as tall seagrass)
            return;
        }

        let Some(item) = Item::from_id(block.item_id) else {
            return;
        };
        let mut stack = ItemStack::new(1, item);

        if pick_item.include_data
            && player.gamemode.load() == GameMode::Creative
            && let Some(block_entity) = world.get_block_entity(&pick_item.pos)
        {
            let mut custom_nbt = pumpkin_nbt::compound::NbtCompound::new();
            custom_nbt.put_string("id", block_entity.resource_location().to_string());
            block_entity.write_nbt(&mut custom_nbt);
            stack.set_data_component(pumpkin_data::data_component_impl::BlockEntityDataImpl {
                nbt: custom_nbt,
            });
        }

        Self::try_pick_item(player, stack);
    }

    pub fn handle_pick_item_from_entity(
        &self,
        player: &Arc<Player>,
        pick_item: &SPickItemFromEntity,
    ) {
        let world = player.world();
        let Some(target) = world.get_entity_by_id(pick_item.id.0) else {
            return;
        };

        let p_eye = player.get_entity().get_eye_pos();
        let t_eye = target.get_eye_pos();
        let dx = p_eye.x - t_eye.x;
        let dy = p_eye.y - t_eye.y;
        let dz = p_eye.z - t_eye.z;
        if dx * dx + dy * dy + dz * dz > 64.0 {
            return;
        }

        if let Some(stack) = target.get_pick_result() {
            Self::try_pick_item(player, stack);
        }
    }

    fn try_pick_item(player: &Arc<Player>, stack: ItemStack) {
        let slot_with_stack = player.inventory().get_slot_with_stack(&stack);

        if slot_with_stack != -1 {
            if PlayerInventory::is_valid_hotbar_index(slot_with_stack as usize) {
                player.inventory.set_selected_slot(slot_with_stack as u8);
            } else {
                player
                    .inventory
                    .swap_slot_with_hotbar(slot_with_stack as usize);
            }
        } else if player.gamemode.load() == GameMode::Creative {
            player.inventory.swap_stack_with_hotbar(stack);
        }

        player.try_send_client_packet(&CSetSelectedSlot::new(
            player.inventory.get_selected_slot() as i8
        ));
        player
            .player_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send_content_updates();
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::entity::EntityType;
    use pumpkin_data::item::Item;
    use pumpkin_data::item_stack::ItemStack;
    use pumpkin_util::math::vector3::Vector3;

    use crate::entity::ai::test_support::world;
    use crate::entity::decoration::cushion::CushionEntity;
    use crate::entity::decoration::item_frame::ItemFrameEntity;
    use crate::entity::{Entity, EntityBase};

    #[tokio::test]
    async fn pick_result_for_vehicles_and_decorations() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let world = world(&temp_dir);

        let boat = Entity::new(
            world.clone(),
            Vector3::new(0.0, 0.0, 0.0),
            &EntityType::OAK_BOAT,
        );
        assert_eq!(
            boat.get_pick_result().map(|s| s.item.id),
            Some(Item::OAK_BOAT.id)
        );

        let minecart = Entity::new(
            world.clone(),
            Vector3::new(0.0, 0.0, 0.0),
            &EntityType::CHEST_MINECART,
        );
        assert_eq!(
            minecart.get_pick_result().map(|s| s.item.id),
            Some(Item::CHEST_MINECART.id)
        );

        let armor_stand = Entity::new(
            world.clone(),
            Vector3::new(0.0, 0.0, 0.0),
            &EntityType::ARMOR_STAND,
        );
        assert_eq!(
            armor_stand.get_pick_result().map(|s| s.item.id),
            Some(Item::ARMOR_STAND.id)
        );

        let lead = Entity::new(
            world.clone(),
            Vector3::new(0.0, 0.0, 0.0),
            &EntityType::LEASH_KNOT,
        );
        assert_eq!(
            lead.get_pick_result().map(|s| s.item.id),
            Some(Item::LEAD.id)
        );

        let cow = Entity::new(world.clone(), Vector3::new(0.0, 0.0, 0.0), &EntityType::COW);
        assert_eq!(
            cow.get_pick_result().map(|s| s.item.id),
            Some(Item::COW_SPAWN_EGG.id)
        );

        let cushion_entity = Entity::new(
            world.clone(),
            Vector3::new(0.0, 0.0, 0.0),
            &EntityType::CUSHION,
        );
        let cushion = CushionEntity::new(cushion_entity, 1);
        assert_eq!(
            cushion.get_pick_result().map(|s| s.item.id),
            Some(Item::ORANGE_CUSHION.id)
        );

        let frame_entity = Entity::new(world, Vector3::new(0.0, 0.0, 0.0), &EntityType::ITEM_FRAME);
        let frame = ItemFrameEntity::new(frame_entity);
        assert_eq!(
            EntityBase::get_pick_result(&frame).map(|s| s.item.id),
            Some(Item::ITEM_FRAME.id)
        );

        frame.set_item(ItemStack::new(1, &Item::DIAMOND), false);
        assert_eq!(
            EntityBase::get_pick_result(&frame).map(|s| s.item.id),
            Some(Item::DIAMOND.id)
        );

        Ok(())
    }
}
