#[allow(clippy::wildcard_imports)]
use super::*;
use pumpkin_protocol::java::server::play::SContainerSlotStateChanged;

impl JavaClient {
    pub fn handle_container_slot_state_changed(
        &self,
        player: &Player,
        packet: &SContainerSlotStateChanged,
    ) {
        if player.is_spectator() {
            return;
        }

        let screen_handler_arc = player
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut screen_handler = screen_handler_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if packet.container_id.0 as u8 != screen_handler.sync_id() {
            return;
        }

        if let Some(crafter) = screen_handler
            .as_any_mut()
            .downcast_mut::<pumpkin_inventory::crafter_screen_handler::CrafterScreenHandler>(
        ) && (0..9).contains(&packet.slot_id.0)
        {
            crafter.set_slot_state(packet.slot_id.0 as usize, packet.new_state);
        }
    }
}
