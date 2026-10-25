use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam::atomic::AtomicCell;

use crate::entity::player::Player;
use crate::entity::vehicle::minecart::container::MinecartInventory;
use crate::entity::vehicle::vehicle::VehicleEntity;
use crate::entity::{Entity, EntityBase, living::LivingEntity};
use crate::server::Server;

use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::Inventory;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::java::client::play::Metadata;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;

pub struct BoatEntity {
    pub vehicle: VehicleEntity,
    ticks_underwater: AtomicCell<f32>,
    left_paddle_moving: AtomicBool,
    right_paddle_moving: AtomicBool,
    pub(crate) inventory: Option<Arc<MinecartInventory>>,
}

impl BoatEntity {
    #[must_use]
    pub const fn is_chest_boat_type(entity_type: &EntityType) -> bool {
        let id = entity_type.id;
        id == EntityType::ACACIA_CHEST_BOAT.id
            || id == EntityType::BIRCH_CHEST_BOAT.id
            || id == EntityType::DARK_OAK_CHEST_BOAT.id
            || id == EntityType::JUNGLE_CHEST_BOAT.id
            || id == EntityType::MANGROVE_CHEST_BOAT.id
            || id == EntityType::OAK_CHEST_BOAT.id
            || id == EntityType::PALE_OAK_CHEST_BOAT.id
            || id == EntityType::SPRUCE_CHEST_BOAT.id
            || id == EntityType::BAMBOO_CHEST_RAFT.id
            || id == EntityType::CHERRY_CHEST_BOAT.id
            || id == EntityType::POPLAR_CHEST_BOAT.id
    }

    pub fn new(mut entity: Entity) -> Self {
        entity.blocks_building = true;
        let is_chest = Self::is_chest_boat_type(entity.entity_type);
        let inventory = is_chest.then(|| Arc::new(MinecartInventory::new(27)));
        Self {
            vehicle: VehicleEntity::new(entity),
            ticks_underwater: AtomicCell::new(0.0),
            left_paddle_moving: AtomicBool::new(false),
            right_paddle_moving: AtomicBool::new(false),
            inventory,
        }
    }

    fn drop_inventory_items(&self) {
        if let Some(inv) = &self.inventory
            && inv.claim_drops()
        {
            let world = self.vehicle.entity.world.load();
            let pos = self.vehicle.entity.pos.load();
            inv.unpack_loot(&crate::world::loot::LootContextParameters {
                world: Some(&world),
                position: Some(pos),
                ..Default::default()
            });
            for slot in 0..inv.size() {
                let stack = inv.get_stack(slot);
                if !stack.is_empty() {
                    world.drop_stack(&pos.to_block_pos(), stack);
                }
            }
        }
    }

    pub fn set_paddles(&self, left: bool, right: bool) {
        self.left_paddle_moving.store(left, Ordering::Relaxed);
        self.right_paddle_moving.store(right, Ordering::Relaxed);

        self.vehicle.entity.send_meta_data(
            &[
                Metadata::new(pumpkin_data::tracked_data::boat::ID_PADDLE_LEFT, left),
                Metadata::new(pumpkin_data::tracked_data::boat::ID_PADDLE_RIGHT, right),
            ],
            None,
        );
    }

    fn send_wobble_metadata(&self) {
        self.vehicle.send_wobble_metadata();
    }
}

impl EntityBase for BoatEntity {
    fn get_entity(&self) -> &Entity {
        &self.vehicle.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }

    fn tick(&self, _caller: &dyn EntityBase, _server: &Server) {
        self.vehicle.tick();

        let underwater = self.ticks_underwater.load();
        if self.vehicle.entity.touching_water.load(Ordering::Relaxed) {
            self.ticks_underwater.store((underwater + 1.0).min(60.0));
        } else if underwater > 0.0 {
            self.ticks_underwater.store((underwater - 1.0).max(0.0));
        }
    }

    fn init_data_tracker(&self) {
        self.send_wobble_metadata();
    }

    fn can_hit(&self) -> bool {
        self.vehicle.entity.is_alive()
    }

    fn is_collidable(&self, _entity: Option<Box<dyn EntityBase>>) -> bool {
        true
    }

    fn damage_with_context(
        &self,
        _caller: &dyn EntityBase,
        amount: f32,
        _damage_type: DamageType,
        _position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        _cause: Option<&dyn EntityBase>,
    ) -> bool {
        let dead = self.vehicle.damage_with_context(amount, source);
        if dead {
            self.drop_inventory_items();
        }
        dead
    }

    fn interact(&self, player: &Arc<Player>, _item_stack: &mut ItemStack) -> bool {
        let is_chest = self.inventory.is_some();

        if is_chest
            && player.get_entity().is_sneaking()
            && let Some(inv) = &self.inventory
        {
            return crate::entity::vehicle::minecart::container::open(
                &self.vehicle.entity,
                player,
                inv,
                TextComponent::translate(pumpkin_data::translation::java::CONTAINER_CHEST, []),
                false,
            );
        }

        if player.get_entity().is_sneaking() {
            return false;
        }

        if self.ticks_underwater.load() >= 60.0 {
            return false;
        }

        let max_passengers = if is_chest { 1 } else { 2 };
        if self
            .vehicle
            .entity
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            >= max_passengers
        {
            if let Some(inv) = &self.inventory {
                return crate::entity::vehicle::minecart::container::open(
                    &self.vehicle.entity,
                    player,
                    inv,
                    TextComponent::translate(pumpkin_data::translation::java::CONTAINER_CHEST, []),
                    false,
                );
            }
            return false;
        }

        if player.get_entity().has_vehicle() {
            return false;
        }

        let world = self.vehicle.entity.world.load();
        let Some(vehicle) = world.get_entity_by_id(self.vehicle.entity.entity_id) else {
            return false;
        };

        let Some(passenger) = world.get_player_by_id(player.entity_id()) else {
            return false;
        };

        self.vehicle
            .entity
            .add_passenger(vehicle, passenger as Arc<dyn EntityBase>);

        true
    }

    fn write_nbt(&self, nbt: &mut NbtCompound) {
        if let Some(inv) = &self.inventory {
            inv.write_nbt(nbt);
        }
    }

    fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        if let Some(inv) = &self.inventory {
            inv.read_nbt(nbt);
        }
    }

    fn set_paddle_state(&self, left: bool, right: bool) {
        self.set_paddles(left, right);
    }
    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_pushable(&self) -> bool {
        true
    }
}
