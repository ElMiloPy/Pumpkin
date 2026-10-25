use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
};

use crossbeam::atomic::AtomicCell;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::tag::{self, Taggable};
use pumpkin_nbt::compound::NbtCompound;
use rand::RngExt;
use uuid::Uuid;

use crate::entity::{
    Entity, EntityBase,
    ageable::{AgeableData, AgeableMob},
    ai::goal::{
        breed::BreedGoal, escape_danger::EscapeDangerGoal, follow_parent::FollowParentGoal,
        look_around::RandomLookAroundGoal, look_at_entity::LookAtEntityGoal, swim::SwimGoal,
        tempt::TemptGoal, wander_around::WanderAroundGoal,
    },
    mob::{Mob, MobEntity},
    passive::animal::Animal,
    player::Player,
};

const TEMPT_ITEMS: &[&Item] = &[
    &Item::GOLDEN_APPLE,
    &Item::ENCHANTED_GOLDEN_APPLE,
    &Item::GOLDEN_CARROT,
];

pub const FLAG_TAME: u8 = 2;
pub const FLAG_SADDLE: u8 = 4;
pub const FLAG_BRED: u8 = 8;
pub const FLAG_EATING: u8 = 16;
pub const FLAG_STANDING: u8 = 32;
pub const FLAG_OPEN_MOUTH: u8 = 64;

pub struct DonkeyEntity {
    pub mob_entity: MobEntity,
    pub ageable_data: AgeableData,
    pub flags: AtomicU8,
    pub has_chest: AtomicBool,
    pub temper: AtomicI32,
    pub owner: AtomicCell<Option<Uuid>>,
}

impl DonkeyEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let mut rng = rand::rng();
        let max_health = 15.0 + rng.random_range(0..8) as f32 + rng.random_range(0..9) as f32;
        mob_entity
            .living_entity
            .set_attribute_base(&Attributes::MAX_HEALTH, f64::from(max_health));
        mob_entity.living_entity.health.store(max_health);
        mob_entity
            .living_entity
            .set_attribute_base(&Attributes::JUMP_STRENGTH, 0.5);
        mob_entity
            .living_entity
            .set_attribute_base(&Attributes::MOVEMENT_SPEED, 0.175);

        let donkey = Self {
            mob_entity,
            ageable_data: AgeableData::default(),
            flags: AtomicU8::new(0),
            has_chest: AtomicBool::new(false),
            temper: AtomicI32::new(0),
            owner: AtomicCell::new(None),
        };
        let mob_arc = Arc::new(donkey);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;

            goal_selector.add_goal(0, Box::new(SwimGoal::default()));
            goal_selector.add_goal(1, EscapeDangerGoal::new(1.2));
            goal_selector.add_goal(2, BreedGoal::new(1.0));
            goal_selector.add_goal(3, Box::new(TemptGoal::new(1.25, TEMPT_ITEMS, false)));
            goal_selector.add_goal(4, Box::new(FollowParentGoal::new(1.0)));
            goal_selector.add_goal(6, Box::new(WanderAroundGoal::new(0.7)));
            goal_selector.add_goal(
                7,
                LookAtEntityGoal::with_default(mob_weak, &EntityType::PLAYER, 6.0),
            );
            goal_selector.add_goal(8, Box::new(RandomLookAroundGoal::default()));
        };

        mob_arc
    }

    #[must_use]
    pub fn has_flag(&self, flag: u8) -> bool {
        (self.flags.load(Ordering::Relaxed) & flag) != 0
    }

    pub fn set_flag(&self, flag: u8, val: bool) {
        let current = self.flags.load(Ordering::Relaxed);
        let new_flags = if val { current | flag } else { current & !flag };
        self.flags.store(new_flags, Ordering::Relaxed);
        let entity = self.get_entity();
        entity.set_synced_data(
            pumpkin_data::tracked_data::donkey::DATA_ID_FLAGS,
            new_flags as i8,
        );
    }

    #[must_use]
    pub fn is_tame(&self) -> bool {
        self.has_flag(FLAG_TAME)
    }

    pub fn set_tame(&self, val: bool) {
        self.set_flag(FLAG_TAME, val);
    }

    #[must_use]
    pub fn is_saddled(&self) -> bool {
        self.has_flag(FLAG_SADDLE)
    }

    pub fn set_saddled(&self, val: bool) {
        self.set_flag(FLAG_SADDLE, val);
    }

    #[must_use]
    pub fn has_chest(&self) -> bool {
        self.has_chest.load(Ordering::Relaxed)
    }

    pub fn set_has_chest(&self, val: bool) {
        self.has_chest.store(val, Ordering::Relaxed);
        let entity = self.get_entity();
        entity.set_synced_data(pumpkin_data::tracked_data::donkey::DATA_ID_CHEST, val);
    }
}

impl AgeableMob for DonkeyEntity {
    fn get_ageable_data(&self) -> &AgeableData {
        &self.ageable_data
    }
}

impl Animal for DonkeyEntity {
    fn is_food(&self, item_stack: &ItemStack) -> bool {
        item_stack.item.has_tag(&tag::Item::MINECRAFT_HORSE_FOOD)
            || item_stack.item == &Item::WHEAT
            || item_stack.item == &Item::SUGAR
            || item_stack.item == &Item::HAY_BLOCK
            || item_stack.item == &Item::APPLE
            || item_stack.item == &Item::GOLDEN_CARROT
            || item_stack.item == &Item::GOLDEN_APPLE
            || item_stack.item == &Item::ENCHANTED_GOLDEN_APPLE
    }
}

impl Mob for DonkeyEntity {
    fn as_ageable(&self) -> Option<&dyn AgeableMob> {
        Some(self)
    }

    fn as_animal(&self) -> Option<&dyn Animal> {
        Some(self)
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.write_ageable_nbt(nbt);
        nbt.put_bool("ChestedHorse", self.has_chest());
        nbt.put_bool("Tame", self.is_tame());
        nbt.put_int("Temper", self.temper.load(Ordering::Relaxed));
        if let Some(owner) = self.owner.load() {
            nbt.put_uuid("Owner", owner);
        }

        let living = &self.mob_entity.living_entity;
        nbt.put_float("Health", living.health.load());
        let mut attr_list = Vec::new();
        let max_health = living.get_max_health();

        let mut h_comp = NbtCompound::new();
        h_comp.put_string("id", "minecraft:max_health".to_string());
        h_comp.put_double("base", f64::from(max_health));
        attr_list.push(pumpkin_nbt::tag::NbtTag::Compound(h_comp));

        nbt.put("attributes", pumpkin_nbt::tag::NbtTag::List(attr_list));
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.read_ageable_nbt(nbt);
        if let Some(chested) = nbt.get_bool("ChestedHorse") {
            self.set_has_chest(chested);
        }
        if let Some(tame) = nbt.get_bool("Tame") {
            self.set_tame(tame);
        }
        if let Some(temper) = nbt.get_int("Temper") {
            self.temper.store(temper, Ordering::Relaxed);
        }
        if let Some(owner) = nbt.get_uuid("Owner") {
            self.owner.store(Some(owner));
        }

        let living = &self.mob_entity.living_entity;
        if let Some(attrs) = nbt
            .get_list("attributes")
            .or_else(|| nbt.get_list("Attributes"))
        {
            for tag in attrs {
                if let pumpkin_nbt::tag::NbtTag::Compound(comp) = tag {
                    let id = comp
                        .get_string("id")
                        .or_else(|| comp.get_string("Name"))
                        .unwrap_or_default();
                    let base = comp
                        .get_double("base")
                        .or_else(|| comp.get_double("Base"))
                        .unwrap_or(0.0);
                    if base > 0.0 && (id == "minecraft:max_health" || id == "generic.max_health") {
                        living.set_attribute_base(&Attributes::MAX_HEALTH, base);
                    }
                }
            }
        }
        if let Some(health) = nbt.get_float("Health") {
            living.health.store(health);
        }
    }

    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn mob_init_data_tracker(&self) {
        let entity = self.get_entity();
        let is_baby = entity.age.load(Ordering::Relaxed) < 0;
        if is_baby {
            entity.set_synced_data(pumpkin_data::tracked_data::donkey::DATA_BABY_ID, true);
        }
        entity.set_synced_data(
            pumpkin_data::tracked_data::donkey::DATA_ID_CHEST,
            self.has_chest(),
        );
        entity.set_synced_data(
            pumpkin_data::tracked_data::donkey::DATA_ID_FLAGS,
            self.flags.load(Ordering::Relaxed) as i8,
        );
    }

    fn custom_server_ai_step(&self, _caller: &dyn EntityBase) {
        if !self.is_tame() && self.get_entity().has_passengers() {
            let entity = self.get_entity();
            let passengers = entity
                .passengers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(passenger) = passengers.first()
                && let Some(player) = passenger.get_player()
                && rand::rng().random_range(0..50) == 0
            {
                let temper = self.temper.load(Ordering::Relaxed);
                if rand::rng().random_range(0..100) < temper {
                    self.set_tame(true);
                    self.owner.store(Some(player.gameprofile.id));
                    let world = entity.world.load();
                    world.send_entity_status(
                        entity,
                        pumpkin_data::entity::EntityStatus::TamingSucceeded,
                        None,
                    );
                    world.play_sound(
                        Sound::EntityDonkeyAmbient,
                        SoundCategory::Neutral,
                        &entity.pos.load(),
                    );
                    return;
                }

                let new_temper = (temper + 5).min(100);
                self.temper.store(new_temper, Ordering::Relaxed);
                let player_id = player.entity_id();
                drop(passengers);
                entity.remove_passenger(player_id);
                let world = entity.world.load();
                world.send_entity_status(
                    entity,
                    pumpkin_data::entity::EntityStatus::TamingFailed,
                    None,
                );
                world.play_sound(
                    Sound::EntityDonkeyAngry,
                    SoundCategory::Neutral,
                    &entity.pos.load(),
                );
            }
        }
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        let item = item_stack.get_item();

        if self.is_tame() && item == &Item::CHEST && !self.has_chest() && !self.is_baby() {
            self.set_has_chest(true);
            item_stack.decrement_unless_creative(player.gamemode.load(), 1);
            let entity = self.get_entity();
            let world = entity.world.load();
            world.play_sound(
                Sound::EntityDonkeyChest,
                SoundCategory::Neutral,
                &entity.pos.load(),
            );
            return true;
        }

        if self.is_tame() && item == &Item::SADDLE && !self.is_saddled() && !self.is_baby() {
            self.set_saddled(true);
            item_stack.decrement_unless_creative(player.gamemode.load(), 1);
            let entity = self.get_entity();
            let world = entity.world.load();
            world.play_sound(
                Sound::EntityHorseSaddle,
                SoundCategory::Neutral,
                &entity.pos.load(),
            );
            return true;
        }

        if self.is_food(item_stack) {
            let (heal, temper_inc) = if item == &Item::WHEAT {
                (2.0f32, 3)
            } else if item == &Item::SUGAR {
                (1.0f32, 3)
            } else if item == &Item::HAY_BLOCK {
                (20.0f32, 0)
            } else if item == &Item::APPLE {
                (3.0f32, 3)
            } else if item == &Item::GOLDEN_CARROT {
                (4.0f32, 5)
            } else if item == &Item::GOLDEN_APPLE || item == &Item::ENCHANTED_GOLDEN_APPLE {
                (10.0f32, 10)
            } else {
                (2.0f32, 3)
            };

            let living = &self.mob_entity.living_entity;
            let current_hp = living.health.load();
            let max_hp = living.get_attribute_value(&Attributes::MAX_HEALTH) as f32;
            let mut used = false;
            if current_hp < max_hp && heal > 0.0 {
                living.set_health((current_hp + heal).min(max_hp));
                used = true;
            }
            let current_temper = self.temper.load(Ordering::Relaxed);
            if temper_inc > 0 && !self.is_tame() && current_temper < 100 {
                self.temper
                    .store((current_temper + temper_inc).min(100), Ordering::Relaxed);
                used = true;
            }
            if self.is_tame()
                && (item == &Item::GOLDEN_CARROT
                    || item == &Item::GOLDEN_APPLE
                    || item == &Item::ENCHANTED_GOLDEN_APPLE)
            {
                return self.animal_interact(player, item_stack, Sound::EntityDonkeyEat);
            }
            if used {
                item_stack.decrement_unless_creative(player.gamemode.load(), 1);
                let entity = self.get_entity();
                entity.world.load().play_sound(
                    Sound::EntityDonkeyEat,
                    SoundCategory::Neutral,
                    &entity.pos.load(),
                );
                return true;
            }
        }

        if !self.is_baby() && !self.is_food(item_stack) {
            let world = player.world();
            let ent = &self.mob_entity.living_entity.entity;
            if let Some(vehicle) = world.get_entity_by_id(ent.entity_id)
                && let Some(passenger) = world.get_player_by_id(player.entity_id())
            {
                ent.add_passenger(vehicle, passenger as Arc<dyn EntityBase>);
                return true;
            }
        }

        self.animal_interact(player, item_stack, Sound::EntityDonkeyAmbient)
    }
}
