use std::sync::{
    Arc, Weak,
    atomic::{AtomicI32, AtomicU8, Ordering},
};

use crossbeam::atomic::AtomicCell;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::tag::{self, Taggable};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::codec::var_int::VarInt;
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

pub struct HorseEntity {
    pub mob_entity: MobEntity,
    pub ageable_data: AgeableData,
    pub variant: AtomicI32,
    pub flags: AtomicU8,
    pub temper: AtomicI32,
    pub owner: AtomicCell<Option<Uuid>>,
}

impl HorseEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let mut rng = rand::rng();
        let color = rng.random_range(0..7);
        let style = rng.random_range(0..5);
        let variant = color | (style << 8);

        // Vanilla AbstractHorse / Horse randomizeAttributes:
        // generateMaxHealth: 15.0 + nextInt(8) + nextInt(9) (15..=30)
        // generateJumpStrength: 0.4 + rand*0.2 + rand*0.2 + rand*0.2 (0.4..=1.0)
        // generateSpeed: (0.45 + rand*0.3 + rand*0.3 + rand*0.3) * 0.25 (0.1125..=0.3375)
        let max_health = 15.0 + rng.random_range(0..8) as f32 + rng.random_range(0..9) as f32;
        let jump_strength = 0.4 + rng.random::<f64>() * 0.2 + rng.random::<f64>() * 0.2 + rng.random::<f64>() * 0.2;
        let speed = (0.45 + rng.random::<f64>() * 0.3 + rng.random::<f64>() * 0.3 + rng.random::<f64>() * 0.3) * 0.25;

        mob_entity.living_entity.set_attribute_base(&Attributes::MAX_HEALTH, f64::from(max_health));
        mob_entity.living_entity.health.store(max_health);
        mob_entity.living_entity.set_attribute_base(&Attributes::JUMP_STRENGTH, jump_strength);
        mob_entity.living_entity.set_attribute_base(&Attributes::MOVEMENT_SPEED, speed);

        let horse = Self {
            mob_entity,
            ageable_data: AgeableData::default(),
            variant: AtomicI32::new(variant),
            flags: AtomicU8::new(0),
            temper: AtomicI32::new(0),
            owner: AtomicCell::new(None),
        };
        let mob_arc = Arc::new(horse);
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
    pub fn get_variant(&self) -> i32 {
        self.variant.load(Ordering::Relaxed)
    }

    pub fn set_variant(&self, variant: i32) {
        self.variant.store(variant, Ordering::Relaxed);
        let entity = self.get_entity();
        entity.set_synced_data(
            pumpkin_data::tracked_data::horse::DATA_ID_TYPE_VARIANT,
            VarInt(variant),
        );
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
            pumpkin_data::tracked_data::horse::DATA_ID_FLAGS,
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
}

impl AgeableMob for HorseEntity {
    fn get_ageable_data(&self) -> &AgeableData {
        &self.ageable_data
    }
}

impl Animal for HorseEntity {
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

    fn get_breed_offspring(&self, partner: &dyn EntityBase) -> Arc<dyn EntityBase> {
        let entity = self.get_entity();
        let world = entity.world.load();
        let child = Self::new(Entity::new(
            world.clone(),
            entity.pos.load(),
            entity.entity_type,
        ));

        // Vanilla horse offspring genetics:
        let mut rng = rand::rng();
        let p1_living = &self.mob_entity.living_entity;
        let p2_living = partner.get_living_entity();

        // Attribute blending: (parent1 + parent2 + random_roll) / 3.0
        let blend_attr = |attr: &Attributes, min: f64, max: f64, rand_val: f64| -> f64 {
            let v1 = p1_living.get_attribute_base(attr);
            let v2 = p2_living.map_or(v1, |p2| p2.get_attribute_base(attr));
            let v3 = min + rand_val * (max - min);
            (v1 + v2 + v3) / 3.0
        };

        let child_health = blend_attr(
            &Attributes::MAX_HEALTH,
            15.0,
            30.0,
            f64::from(rng.random_range(0..16)) / 15.0,
        ) as f32;
        let child_jump = blend_attr(
            &Attributes::JUMP_STRENGTH,
            0.4,
            1.0,
            rng.random::<f64>(),
        );
        let child_speed = blend_attr(
            &Attributes::MOVEMENT_SPEED,
            0.1125,
            0.3375,
            rng.random::<f64>(),
        );

        let child_living = &child.mob_entity.living_entity;
        child_living.set_attribute_base(&Attributes::MAX_HEALTH, f64::from(child_health));
        child_living.health.store(child_health);
        child_living.set_attribute_base(&Attributes::JUMP_STRENGTH, child_jump);
        child_living.set_attribute_base(&Attributes::MOVEMENT_SPEED, child_speed);

        // Variant inheritance per vanilla:
        let p1_var = self.get_variant();
        let p2_var = partner
            .cast_any()
            .downcast_ref::<Self>()
            .map_or(p1_var, Self::get_variant);
        let child_color = match rng.random_range(0..9) {
            0..4 => p1_var & 0xFF,
            4..8 => p2_var & 0xFF,
            _ => rng.random_range(0..7),
        };
        let child_style = match rng.random_range(0..9) {
            0..4 => (p1_var >> 8) & 0xFF,
            4..8 => (p2_var >> 8) & 0xFF,
            _ => rng.random_range(0..5),
        };
        child.set_variant(child_color | (child_style << 8));

        child
    }
}

impl Mob for HorseEntity {
    fn as_ageable(&self) -> Option<&dyn AgeableMob> {
        Some(self)
    }

    fn as_animal(&self) -> Option<&dyn Animal> {
        Some(self)
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.write_ageable_nbt(nbt);
        nbt.put_int("Variant", self.get_variant());
        nbt.put_bool("Tame", self.is_tame());
        nbt.put_int("Temper", self.temper.load(Ordering::Relaxed));
        if let Some(owner) = self.owner.load() {
            nbt.put_uuid("Owner", owner);
        }

        let living = &self.mob_entity.living_entity;
        nbt.put_float("Health", living.health.load());
        let mut attr_list = Vec::new();
        let max_health = living.get_max_health();
        let jump = living.get_attribute_base(&Attributes::JUMP_STRENGTH);
        let speed = living.get_attribute_base(&Attributes::MOVEMENT_SPEED);

        let mut h_comp = NbtCompound::new();
        h_comp.put_string("id", "minecraft:max_health".to_string());
        h_comp.put_double("base", f64::from(max_health));
        attr_list.push(pumpkin_nbt::tag::NbtTag::Compound(h_comp));

        let mut j_comp = NbtCompound::new();
        j_comp.put_string("id", "minecraft:jump_strength".to_string());
        j_comp.put_double("base", jump);
        attr_list.push(pumpkin_nbt::tag::NbtTag::Compound(j_comp));

        let mut s_comp = NbtCompound::new();
        s_comp.put_string("id", "minecraft:movement_speed".to_string());
        s_comp.put_double("base", speed);
        attr_list.push(pumpkin_nbt::tag::NbtTag::Compound(s_comp));

        nbt.put("attributes", pumpkin_nbt::tag::NbtTag::List(attr_list));
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.read_ageable_nbt(nbt);
        if let Some(variant) = nbt.get_int("Variant") {
            self.set_variant(variant);
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
        if let Some(attrs) = nbt.get_list("attributes").or_else(|| nbt.get_list("Attributes")) {
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
                    if base > 0.0 {
                        if id == "minecraft:max_health" || id == "generic.max_health" {
                            living.set_attribute_base(&Attributes::MAX_HEALTH, base);
                        } else if id == "minecraft:jump_strength" || id == "horse.jump_strength" {
                            living.set_attribute_base(&Attributes::JUMP_STRENGTH, base);
                        } else if id == "minecraft:movement_speed"
                            || id == "generic.movement_speed"
                        {
                            living.set_attribute_base(&Attributes::MOVEMENT_SPEED, base);
                        }
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
            entity.set_synced_data(pumpkin_data::tracked_data::horse::DATA_BABY_ID, true);
        }
        entity.set_synced_data(
            pumpkin_data::tracked_data::horse::DATA_ID_TYPE_VARIANT,
            VarInt(self.get_variant()),
        );
        entity.set_synced_data(
            pumpkin_data::tracked_data::horse::DATA_ID_FLAGS,
            self.flags.load(Ordering::Relaxed) as i8,
        );
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        let item = item_stack.get_item();

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

        self.animal_interact(player, item_stack, Sound::EntityHorseAmbient)
    }
}
