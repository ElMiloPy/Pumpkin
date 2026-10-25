use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pumpkin_data::entity::EntityType;
use pumpkin_data::game_event::GameEvent;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_nbt::compound::NbtCompound;
use rand::RngExt;

use crate::entity::{
    Entity,
    custom_sound::CustomSound,
    item::ItemEntity,
    mob::{Mob, MobEntity, skeleton::SkeletonEntityBase, spawn::SpawnGroupData},
    player::Player,
    projectile::arrow::ArrowEntity,
};
use crate::world::World;
use crate::world::loot::{LootContextParameters, generate_loot_from_handle};

pub struct BoggedSkeletonEntity {
    entity: Arc<SkeletonEntityBase>,
    pub sheared: AtomicBool,
}

impl BoggedSkeletonEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let base = SkeletonEntityBase::new_with_intervals(entity, 70, 50);
        base.mob_entity.living_entity.set_max_health(16.0);
        base.mob_entity.living_entity.set_health(16.0);
        let bogged = Self {
            entity: base,
            sheared: AtomicBool::new(false),
        };
        Arc::new(bogged)
    }

    pub fn is_sheared(&self) -> bool {
        self.sheared.load(Ordering::Relaxed)
    }

    pub fn set_sheared(&self, sheared: bool) {
        self.sheared.store(sheared, Ordering::Relaxed);
        self.entity
            .mob_entity
            .living_entity
            .entity
            .set_synced_data(pumpkin_data::tracked_data::bogged::DATA_SHEARED, sheared);
    }

    pub fn shear(&self, category: SoundCategory, tool: &ItemStack) {
        let entity = &self.entity.mob_entity.living_entity.entity;
        let world = entity.world.load();
        let pos = entity.pos.load();
        world.play_sound(Sound::EntityBoggedShear, category, &pos);
        if let Some(table) = world.get_loot_table("minecraft:shearing/bogged") {
            let params = LootContextParameters {
                world: Some(&world),
                position: Some(pos),
                this_entity: Some(self),
                tool: Some(tool.clone()),
                ..Default::default()
            };
            let mut rng = rand::rng();
            for stack in generate_loot_from_handle(&table, rng.random(), &params) {
                for _ in 0..stack.item_count {
                    let item_entity = ItemEntity::new(
                        Entity::new(
                            world.clone(),
                            pos.add_raw(0.0, entity.get_eye_height(), 0.0),
                            &EntityType::ITEM,
                        ),
                        stack.copy_with_count(1),
                    );
                    world.spawn_entity(Arc::new(item_entity));
                }
            }
        }
        self.set_sheared(true);
    }
}

impl CustomSound for BoggedSkeletonEntity {
    fn death_sound(&self) -> Option<Sound> {
        Some(Sound::EntityBoggedDeath)
    }

    fn hurt_sound(&self) -> Option<Sound> {
        Some(Sound::EntityBoggedHurt)
    }

    fn sound_category(&self) -> SoundCategory {
        SoundCategory::Hostile
    }
}

impl Mob for BoggedSkeletonEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn as_custom_sound(&self) -> Option<&dyn CustomSound> {
        Some(self)
    }

    fn shoot_sound(&self) -> Option<Sound> {
        Some(Sound::EntitySkeletonShoot)
    }

    fn get_arrow(&self, arrow: ArrowEntity) -> ArrowEntity {
        arrow.add_effect(pumpkin_data::data_component_impl::StatusEffectInstance {
            effect_id: "poison".into(),
            amplifier: 0,
            duration: 100,
            ambient: false,
            show_particles: true,
            show_icon: true,
        });
        arrow
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        if item_stack.get_item() == &Item::SHEARS && !self.is_sheared() {
            let entity = &self.entity.mob_entity.living_entity.entity;
            let world = entity.world.load();
            if let Some(server) = world.server.upgrade() {
                let mut event =
                    crate::plugin::api::events::player::player_shear_entity::PlayerShearEntityEvent {
                        player: player.clone(),
                        entity_id: entity.entity_id,
                        hand: 0,
                        cancelled: false,
                    };
                server.plugin_manager.fire_blocking(&server, &mut event);
                if event.cancelled {
                    return false;
                }
            }
            self.shear(SoundCategory::Players, item_stack);
            world.emit_game_event(GameEvent::Shear.name(), entity.pos.load());
            let _ = player.damage_item_stack(item_stack, 1);
            return true;
        }
        self.entity.mob_interact(player, item_stack)
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_bool("sheared", self.is_sheared());
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        let sheared = nbt.get_bool("sheared").unwrap_or(false);
        self.set_sheared(sheared);
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.entity.finalize_spawn(world, group_data)
    }
}
