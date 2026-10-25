use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{
    entity::EntityType,
    item::Item,
    sound::{Sound, SoundCategory},
    tag::{self, Taggable},
};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::codec::var_int::VarInt;

use crate::entity::{
    Entity, EntityBase,
    ai::goal::{
        active_target::ActiveTargetGoal, avoid_entity::AvoidEntityGoal,
        creeper_ignite::CreeperIgniteGoal, look_around::RandomLookAroundGoal,
        look_at_entity::LookAtEntityGoal, melee_attack::MeleeAttackGoal, revenge::RevengeGoal,
        swim::SwimGoal, wander_around::WanderAroundGoal,
    },
    area_effect_cloud::AreaEffectCloudEntity,
    mob::{Mob, MobEntity, default_set_mob_target},
    player::Player,
};
use crate::world::loot::LootContextParameters;

const DEFAULT_FUSE_TIME: i32 = 30;
const DEFAULT_EXPLOSION_RADIUS: i32 = 3;

pub struct CreeperEntity {
    pub mob_entity: MobEntity,
    pub fuse_speed: AtomicI32,
    pub current_fuse_time: AtomicI32,
    pub last_fuse_time: AtomicI32,
    pub fuse_time: AtomicI32,
    pub explosion_radius: AtomicI32,
    pub ignited: AtomicBool,
    pub charged: AtomicBool,
    pub dropped_skulls: AtomicBool,
}

impl CreeperEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let entity = Self {
            mob_entity,
            fuse_speed: AtomicI32::new(-1),
            current_fuse_time: AtomicI32::new(0),
            last_fuse_time: AtomicI32::new(0),
            fuse_time: AtomicI32::new(DEFAULT_FUSE_TIME),
            explosion_radius: AtomicI32::new(DEFAULT_EXPLOSION_RADIUS),
            ignited: AtomicBool::new(false),
            charged: AtomicBool::new(false),
            dropped_skulls: AtomicBool::new(false),
        };
        let mob_arc = Arc::new(entity);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;
            let target_selector = &mob_arc.mob_entity.target_selector;

            goal_selector.add_goal(1, Box::new(SwimGoal::default()));
            goal_selector.add_goal(2, Box::new(CreeperIgniteGoal::new(mob_arc.clone())));
            goal_selector.add_goal(
                3,
                Box::new(AvoidEntityGoal::new(&EntityType::OCELOT, 6.0, 1.0, 1.2)),
            );
            goal_selector.add_goal(
                3,
                Box::new(AvoidEntityGoal::new(&EntityType::CAT, 6.0, 1.0, 1.2)),
            );
            goal_selector.add_goal(4, Box::new(MeleeAttackGoal::new(1.0, false)));
            goal_selector.add_goal(5, Box::new(WanderAroundGoal::new(0.8)));

            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak, &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(6, Box::new(RandomLookAroundGoal::default()));

            target_selector.add_goal(
                1,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::PLAYER, true),
            );
            target_selector.add_goal(2, Box::new(RevengeGoal::new(true)));
        };

        mob_arc
    }

    pub fn set_fuse_speed(&self, speed: i32) {
        self.fuse_speed.store(speed, Ordering::Relaxed);
        self.mob_entity
            .living_entity
            .entity
            .set_synced_data(pumpkin_data::tracked_data::creeper::FUSE_ID, VarInt(speed));
    }

    pub fn explode(&self) {
        let entity = &self.mob_entity.living_entity.entity;
        let radius = self.explosion_radius.load(Ordering::Relaxed) as f32;
        let multiplier = if self.charged.load(Ordering::Relaxed) {
            2.0
        } else {
            1.0
        };
        self.mob_entity
            .living_entity
            .dead
            .store(true, Ordering::Relaxed);
        let world = entity.world.load();
        let pos = entity.pos.load();
        let source = world.get_entity_by_id(entity.entity_id);
        world.explode_with_source(
            pos,
            radius * multiplier,
            crate::world::ExplosionInteraction::Mob,
            source,
        );
        self.spawn_lingering_cloud();
        entity.remove();
    }

    fn spawn_lingering_cloud(&self) {
        let living = &self.mob_entity.living_entity;
        let Ok(effects_guard) = living.active_effects.try_lock() else {
            return;
        };
        if effects_guard.is_empty() {
            return;
        }

        let mut cloud_effects = Vec::new();
        for effect in effects_guard.values() {
            let scaled_duration = (effect.duration as f32 * 0.25).max(1.0) as i32;
            cloud_effects.push((
                effect.effect_type,
                scaled_duration,
                effect.amplifier,
                effect.ambient,
                effect.show_particles,
                effect.show_icon,
            ));
        }
        drop(effects_guard);

        let entity = &living.entity;
        let world = entity.world.load();
        let cloud_entity = Entity::from_uuid(
            uuid::Uuid::new_v4(),
            world.clone(),
            entity.pos.load(),
            &EntityType::AREA_EFFECT_CLOUD,
        );
        let cloud = AreaEffectCloudEntity::create(
            cloud_entity,
            ItemStack::EMPTY.clone(),
            cloud_effects,
            300,
            2.5,
            20,
            10,
            -0.5,
            0,
        );
        world.spawn_entity(cloud);
    }
}

impl Mob for CreeperEntity {
    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_bool("powered", self.charged.load(Ordering::Relaxed));
        nbt.put_short("Fuse", self.fuse_time.load(Ordering::Relaxed) as i16);
        nbt.put_byte(
            "ExplosionRadius",
            self.explosion_radius.load(Ordering::Relaxed) as i8,
        );
        nbt.put_bool("ignited", self.ignited.load(Ordering::Relaxed));
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        if let Some(powered) = nbt.get_bool("powered") {
            self.charged.store(powered, Ordering::Relaxed);
        }
        if let Some(fuse) = nbt.get_short("Fuse") {
            self.fuse_time.store(i32::from(fuse), Ordering::Relaxed);
        }
        if let Some(radius) = nbt.get_byte("ExplosionRadius") {
            self.explosion_radius
                .store(i32::from(radius), Ordering::Relaxed);
        }
        if let Some(ignited) = nbt.get_bool("ignited") {
            self.ignited.store(ignited, Ordering::Relaxed);
        }
    }

    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn mob_metadata(&self) -> Option<crate::entity::mob::MobMetadata> {
        let mut meta = crate::entity::mob::MobMetadata::new();
        if self.is_charged() {
            meta = meta.charged(true);
        }
        if self.is_ignited() {
            meta = meta.ignited(true);
        }
        let fuse = self.fuse_speed.load(Ordering::Relaxed);
        if fuse != -1 {
            meta = meta.fuse_speed(fuse);
        }
        Some(meta)
    }

    fn mob_on_lightning_strike(
        &self,
        caller: &dyn EntityBase,
        lightning: &crate::entity::lightning::LightningBoltEntity,
    ) {
        self.charged.store(true, Ordering::Relaxed);
        self.mob_entity
            .living_entity
            .entity
            .set_synced_data(pumpkin_data::tracked_data::creeper::CHARGED, true);
        self.mob_entity
            .living_entity
            .on_lightning_strike(caller, lightning);
    }

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        let entity = &self.mob_entity.living_entity.entity;
        if !entity.is_alive() {
            return;
        }

        self.last_fuse_time.store(
            self.current_fuse_time.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

        if self.ignited.load(Ordering::Relaxed) {
            self.set_fuse_speed(1);
        }

        let fuse_speed = self.fuse_speed.load(Ordering::Relaxed);
        let current = self.current_fuse_time.load(Ordering::Relaxed);

        if fuse_speed > 0 && current == 0 {
            let world = entity.world.load();
            world.play_sound_fine(
                Sound::EntityCreeperPrimed,
                SoundCategory::Hostile,
                &entity.pos.load(),
                1.0,
                0.5,
            );
        }

        let fuse_time = self.fuse_time.load(Ordering::Relaxed);
        let new_fuse = (current + fuse_speed).max(0);
        self.current_fuse_time.store(new_fuse, Ordering::Relaxed);

        if new_fuse >= fuse_time {
            self.current_fuse_time.store(fuse_time, Ordering::Relaxed);
            self.explode();
        }
    }

    fn can_attack(&self, target: &dyn EntityBase) -> bool {
        if target.get_entity().entity_type == &EntityType::GOAT {
            return false;
        }
        self.mob_entity.living_entity.can_attack(target)
    }

    fn set_mob_target(&self, target: Option<Arc<dyn EntityBase>>) {
        if let Some(ref t) = target
            && t.get_entity().entity_type == &EntityType::GOAT
        {
            return;
        }
        let target = self.as_valid_target(target);
        default_set_mob_target(self.get_mob_entity(), target);
    }

    fn mob_cause_fall_damage(&self, fall_distance: f32, _damage_multiplier: f32) {
        let max_swell = self.fuse_time.load(Ordering::Relaxed);
        let mut swell = self.current_fuse_time.load(Ordering::Relaxed);
        swell += (fall_distance * 1.5) as i32;
        if swell > max_swell - 5 {
            swell = max_swell - 5;
        }
        self.current_fuse_time
            .store(swell.max(0), Ordering::Relaxed);
    }

    fn mob_killed_entity(&self, victim: &dyn EntityBase, params: &LootContextParameters<'_>) {
        let world = self.mob_entity.living_entity.entity.world.load();
        if self.is_charged()
            && world.level_info.load().game_rules.mob_drops
            && self
                .dropped_skulls
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            && let Some(table) = world.get_loot_table("minecraft:charged_creeper/root")
        {
            let seed: i64 = rand::random();
            let pos = victim.get_entity().block_pos.load();
            for stack in crate::world::loot::generate_loot_from_handle(&table, seed, params) {
                world.drop_stack(&pos, stack);
            }
        }
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        if !item_stack
            .item
            .has_tag(&tag::Item::MINECRAFT_CREEPER_IGNITERS)
        {
            return self
                .mob_entity
                .mob_interact(player, item_stack, self.can_be_leashed());
        }

        let entity = &self.mob_entity.living_entity.entity;
        let world = entity.world.load();
        let pos = entity.pos.load();

        let sound = if item_stack.item.id == Item::FIRE_CHARGE.id {
            Sound::ItemFirechargeUse
        } else {
            Sound::ItemFlintandsteelUse
        };

        world.play_sound_fine(
            sound,
            SoundCategory::Hostile,
            &pos,
            1.0,
            rand::random::<f32>() * 0.4 + 0.8,
        );

        self.set_ignited(true);

        if player.gamemode.load() != pumpkin_util::GameMode::Creative {
            if item_stack.is_damageable() {
                let _ = item_stack.damage_item(1);
            } else {
                item_stack.item_count = item_stack.item_count.saturating_sub(1);
            }
        }

        true
    }
}

impl CreeperEntity {
    pub fn is_charged(&self) -> bool {
        self.charged.load(Ordering::Relaxed)
    }

    pub fn set_charged(&self, charged: bool) {
        self.charged.store(charged, Ordering::Relaxed);
        let entity = &self.mob_entity.living_entity.entity;
        entity.set_synced_data(pumpkin_data::tracked_data::creeper::CHARGED, charged);
        if let Some(meta) = self.mob_metadata() {
            entity.send_bedrock_actor_data(&meta.to_bedrock_actor_data());
        }
    }

    pub fn is_ignited(&self) -> bool {
        self.ignited.load(Ordering::Relaxed)
    }

    pub fn set_ignited(&self, ignited: bool) {
        self.ignited.store(ignited, Ordering::Relaxed);
        let entity = &self.mob_entity.living_entity.entity;
        entity.set_synced_data(pumpkin_data::tracked_data::creeper::IS_IGNITED, ignited);
        if let Some(meta) = self.mob_metadata() {
            entity.send_bedrock_actor_data(&meta.to_bedrock_actor_data());
        }
    }

    pub fn get_fuse(&self) -> i32 {
        self.fuse_time.load(Ordering::Relaxed)
    }

    pub fn set_fuse(&self, fuse: i32) {
        self.fuse_time.store(fuse, Ordering::Relaxed);
    }

    pub fn get_explosion_radius(&self) -> i32 {
        self.explosion_radius.load(Ordering::Relaxed)
    }

    pub fn set_explosion_radius(&self, radius: i32) {
        self.explosion_radius.store(radius, Ordering::Relaxed);
    }
}
