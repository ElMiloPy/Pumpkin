use std::any::Any;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::screen::WindowType;

use crate::crafting::crafting_screen_handler::match_crafting_recipe;
use crate::crafting::recipes::RecipeInputInventory;
use crate::inventory::{Inventory, SimpleInventory};
use crate::player::player_inventory::PlayerInventory;
use crate::screen_handler::{
    InventoryPlayer, ScreenHandler, ScreenHandlerBehaviour, ScreenHandlerListener, ScreenProperty,
};
use crate::slot::Slot;
use crate::window_property::PropertyDelegate;

pub struct CrafterSlot {
    inventory: Arc<dyn Inventory>,
    property_delegate: Arc<dyn PropertyDelegate>,
    index: usize,
    id: AtomicU8,
}

impl CrafterSlot {
    #[must_use]
    pub fn new(
        inventory: Arc<dyn Inventory>,
        property_delegate: Arc<dyn PropertyDelegate>,
        index: usize,
    ) -> Self {
        Self {
            inventory,
            property_delegate,
            index,
            id: AtomicU8::new(0),
        }
    }

    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.property_delegate.get_property(self.index as i32) == 1
    }
}

impl Slot for CrafterSlot {
    fn get_inventory(&self) -> Arc<dyn Inventory> {
        self.inventory.clone()
    }

    fn get_index(&self) -> usize {
        self.index
    }

    fn set_id(&self, index: usize) {
        self.id.store(index as u8, Ordering::Relaxed);
    }

    fn can_insert(&self, stack: &ItemStack) -> bool {
        !self.is_disabled() && self.inventory.is_valid_slot_for(self.index, stack)
    }

    fn mark_dirty(&self) {
        self.inventory.mark_dirty();
    }
}

pub struct CrafterResultSlot {
    inventory: Arc<dyn RecipeInputInventory>,
    result_inventory: Arc<SimpleInventory>,
    result: Arc<Mutex<ItemStack>>,
    id: AtomicU8,
}

impl CrafterResultSlot {
    #[must_use]
    pub fn new(inventory: Arc<dyn RecipeInputInventory>) -> Self {
        let result_inventory = Arc::new(SimpleInventory::new(1));
        Self {
            inventory,
            result_inventory,
            result: Arc::new(Mutex::new(ItemStack::EMPTY.clone())),
            id: AtomicU8::new(0),
        }
    }

    pub fn refresh_result(&self) -> ItemStack {
        let recipe = match_crafting_recipe(self.inventory.as_ref(), None);
        let stack = recipe.map_or_else(|| ItemStack::EMPTY.clone(), |r| r.to_item_stack());
        *self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = stack.clone();
        self.result_inventory.set_stack(0, stack.clone());
        stack
    }
}

impl Slot for CrafterResultSlot {
    fn get_inventory(&self) -> Arc<dyn Inventory> {
        self.result_inventory.clone()
    }

    fn get_index(&self) -> usize {
        0
    }

    fn set_id(&self, index: usize) {
        self.id.store(index as u8, Ordering::Relaxed);
    }

    fn can_insert(&self, _stack: &ItemStack) -> bool {
        false
    }

    fn can_take_items(&self, _player: &dyn InventoryPlayer) -> bool {
        false
    }

    fn allow_modification(&self, _player: &dyn InventoryPlayer) -> bool {
        false
    }

    fn take_stack(&self, _amount: u8) -> ItemStack {
        ItemStack::EMPTY.clone()
    }

    fn mark_dirty(&self) {
        self.result_inventory.mark_dirty();
    }
}

impl ScreenHandlerListener for CrafterResultSlot {
    fn on_slot_update(&self, screen_handler: &ScreenHandlerBehaviour, slot: u8, _stack: ItemStack) {
        if slot < 9 {
            let result = self.refresh_result();
            let next_revision = screen_handler.next_revision();
            if let Some(sync_handler) = screen_handler.sync_handler.as_ref() {
                sync_handler.update_slot(screen_handler, 45, &result, next_revision);
            }
        }
    }
}

struct CrafterScreenListener;

impl ScreenHandlerListener for CrafterScreenListener {
    fn on_property_update(
        &self,
        screen_handler: &ScreenHandlerBehaviour,
        property: u8,
        value: i32,
    ) {
        if let Some(sync_handler) = screen_handler.sync_handler.as_ref() {
            sync_handler.update_property(screen_handler, i32::from(property), value);
        }
    }
}

pub struct CrafterScreenHandler {
    behaviour: ScreenHandlerBehaviour,
    pub crafter_inventory: Arc<dyn Inventory>,
    pub property_delegate: Arc<dyn PropertyDelegate>,
    pub result_slot: Arc<CrafterResultSlot>,
    pub is_spectator: bool,
}

impl CrafterScreenHandler {
    #[must_use]
    pub fn new(
        sync_id: u8,
        player_inventory: &Arc<PlayerInventory>,
        crafter_inventory: &Arc<dyn Inventory>,
        property_delegate: &Arc<dyn PropertyDelegate>,
        recipe_input: Arc<dyn RecipeInputInventory>,
        is_spectator: bool,
    ) -> Self {
        let result_slot = Arc::new(CrafterResultSlot::new(recipe_input));
        result_slot.refresh_result();

        let mut handler = Self {
            behaviour: ScreenHandlerBehaviour::new(sync_id, Some(WindowType::Crafter3x3)),
            crafter_inventory: crafter_inventory.clone(),
            property_delegate: property_delegate.clone(),
            result_slot: result_slot.clone(),
            is_spectator,
        };

        // 1. Crafter slots (0..9)
        for i in 0..9 {
            handler.add_slot(Arc::new(CrafterSlot::new(
                crafter_inventory.clone(),
                property_delegate.clone(),
                i,
            )));
        }

        // 2. Player inventory (9..36) and hotbar (36..45)
        let player_inv: Arc<dyn Inventory> = player_inventory.clone();
        handler.add_player_slots(&player_inv);

        // 3. Result preview slot (45)
        handler.add_slot(result_slot.clone());

        // 4. Tracked properties (0..=8: slot disabled states, 9: triggered)
        for i in 0..10 {
            handler.add_property(ScreenProperty::new(property_delegate.clone(), i));
        }

        handler.add_listener(Arc::new(CrafterScreenListener));
        handler.add_listener(result_slot);

        handler
    }

    pub fn set_slot_state(&mut self, slot_id: usize, enabled: bool) {
        if slot_id < 9 {
            let value = i32::from(!enabled);
            self.property_delegate.set_property(slot_id as i32, value);
            self.update_tracked_properties(slot_id as i32, value);
            if let Some(sync_handler) = self.get_behaviour().sync_handler.as_ref() {
                sync_handler.update_property(self.get_behaviour(), slot_id as i32, value);
            }
            let result = self.result_slot.refresh_result();
            let next_revision = self.get_behaviour().next_revision();
            if let Some(sync_handler) = self.get_behaviour().sync_handler.as_ref() {
                sync_handler.update_slot(self.get_behaviour(), 45, &result, next_revision);
            }
        }
    }
}

impl ScreenHandler for CrafterScreenHandler {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_behaviour(&self) -> &ScreenHandlerBehaviour {
        &self.behaviour
    }

    fn get_behaviour_mut(&mut self) -> &mut ScreenHandlerBehaviour {
        &mut self.behaviour
    }

    fn on_closed(&mut self, player: &dyn InventoryPlayer) {
        self.default_on_closed(player);
    }

    fn quick_move(&mut self, player: &dyn InventoryPlayer, slot_index: i32) -> ItemStack {
        if slot_index < 0 || slot_index as usize >= self.get_behaviour().slots.len() {
            return ItemStack::EMPTY.clone();
        }
        let slot = self.get_behaviour().slots[slot_index as usize].clone();
        if !slot.has_stack() {
            return ItemStack::EMPTY.clone();
        }

        let mut stack = slot.get_stack();
        let clicked = stack.clone();

        if slot_index < 9 {
            // slots 9..45: player inventory (27) + player hotbar (9)
            if !self.insert_item(&mut stack, 9, 45, true) {
                return ItemStack::EMPTY.clone();
            }
        } else if slot_index < 45 {
            // slots 0..9: crafter slots
            if !self.insert_item(&mut stack, 0, 9, false) {
                return ItemStack::EMPTY.clone();
            }
        } else {
            // slot 45: result slot cannot be quick moved
            return ItemStack::EMPTY.clone();
        }

        if stack.is_empty() {
            slot.set_stack(ItemStack::EMPTY.clone());
        } else {
            slot.mark_dirty();
        }

        if stack.item_count == clicked.item_count {
            return ItemStack::EMPTY.clone();
        }

        slot.on_take_item(player, &stack);
        let result = self.result_slot.refresh_result();
        let next_revision = self.get_behaviour().next_revision();
        if let Some(sync_handler) = self.get_behaviour().sync_handler.as_ref() {
            sync_handler.update_slot(self.get_behaviour(), 45, &result, next_revision);
        }
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crafting::crafting_inventory::CraftingInventory;
    use crate::entity_equipment::EntityEquipment;
    use crate::inventory::Clearable;
    use pumpkin_data::item::Item;
    use std::sync::RwLock;

    struct MockCrafterInventory {
        inner: CraftingInventory,
        disabled_slots: RwLock<[bool; 9]>,
        triggered: std::sync::atomic::AtomicBool,
    }

    impl MockCrafterInventory {
        fn new() -> Self {
            Self {
                inner: CraftingInventory::new(3, 3),
                disabled_slots: RwLock::new([false; 9]),
                triggered: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    impl Clearable for MockCrafterInventory {
        fn clear(&self) {
            self.inner.clear();
        }
    }

    impl Inventory for MockCrafterInventory {
        fn size(&self) -> usize {
            9
        }
        fn is_empty(&self) -> bool {
            self.inner.is_empty()
        }
        fn get_stack(&self, slot: usize) -> ItemStack {
            self.inner.get_stack(slot)
        }
        fn remove_stack(&self, slot: usize) -> ItemStack {
            self.inner.remove_stack(slot)
        }
        fn remove_stack_specific(&self, slot: usize, amount: u8) -> ItemStack {
            self.inner.remove_stack_specific(slot, amount)
        }
        fn set_stack(&self, slot: usize, stack: ItemStack) {
            self.inner.set_stack(slot, stack);
        }
        fn is_valid_slot_for(&self, slot: usize, _stack: &ItemStack) -> bool {
            !self.disabled_slots.read().unwrap()[slot]
        }
        fn mark_dirty(&self) {}
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl RecipeInputInventory for MockCrafterInventory {
        fn get_width(&self) -> usize {
            3
        }
        fn get_height(&self) -> usize {
            3
        }
    }

    impl PropertyDelegate for MockCrafterInventory {
        fn get_property(&self, index: i32) -> i32 {
            match index {
                0..=8 => i32::from(self.disabled_slots.read().unwrap()[index as usize]),
                9 => i32::from(self.triggered.load(Ordering::Relaxed)),
                _ => 0,
            }
        }
        fn set_property(&self, index: i32, value: i32) {
            match index {
                0..=8 => self.disabled_slots.write().unwrap()[index as usize] = value != 0,
                9 => self.triggered.store(value != 0, Ordering::Relaxed),
                _ => {}
            }
        }
        fn get_properties_size(&self) -> i32 {
            10
        }
    }

    #[test]
    fn test_crafter_screen_handler_slot_toggle_and_disabled_state() {
        let player_inv = Arc::new(PlayerInventory::new(
            Arc::new(Mutex::new(EntityEquipment::new())),
            Arc::new(rustc_hash::FxHashMap::default()),
        ));
        let mock = Arc::new(MockCrafterInventory::new());
        let mut handler =
            CrafterScreenHandler::new(1, &player_inv, &mock, &mock, mock.clone(), false);

        assert_eq!(handler.get_behaviour().slots.len(), 46);
        assert_eq!(mock.get_property(2), 0);

        // Disable slot 2
        handler.set_slot_state(2, false);
        assert_eq!(mock.get_property(2), 1);

        // CrafterSlot 2 should now reject items
        let slot2 = handler.get_behaviour().slots[2].clone();
        assert!(!slot2.can_insert(&ItemStack::new(1, &Item::DIRT)));

        // CrafterSlot 0 should accept items
        let slot0 = handler.get_behaviour().slots[0].clone();
        assert!(slot0.can_insert(&ItemStack::new(1, &Item::DIRT)));

        // Re-enable slot 2
        handler.set_slot_state(2, true);
        assert_eq!(mock.get_property(2), 0);
        assert!(slot2.can_insert(&ItemStack::new(1, &Item::DIRT)));
    }
}
