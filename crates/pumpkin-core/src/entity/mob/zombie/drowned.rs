use std::sync::{Arc, Weak};

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_nbt::compound::NbtCompound;

use crate::entity::ai::goal::active_target::ActiveTargetGoal;
use crate::entity::ai::goal::ranged_attack::RangedAttackGoal;
use crate::entity::ai::goal::zombie_attack::ZombieAttackGoal;
use crate::entity::ai::goal::{Controls, Goal};
use crate::entity::mob::equipment::RegionalDifficulty;
use crate::entity::mob::spawn::SpawnGroupData;
use crate::entity::mob::zombie::ZombieEntityBase;
use crate::entity::mob::{Mob, MobEntity, RangedAttackMob};
use crate::entity::projectile::arrow::ArrowPickup;
use crate::entity::projectile::trident::TridentEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

fn is_holding_trident(mob: &dyn Mob) -> bool {
    mob.get_mob_entity()
        .living_entity
        .entity_equipment
        .try_lock()
        .is_ok_and(|eq| eq.get(&EquipmentSlot::MAIN_HAND).item.id == Item::TRIDENT.id)
}

fn is_not_holding_trident(mob: &dyn Mob) -> bool {
    !is_holding_trident(mob)
}

pub struct DrownedTridentAttackGoal {
    inner: RangedAttackGoal,
}

impl DrownedTridentAttackGoal {
    #[must_use]
    pub const fn new(ranged_goal: RangedAttackGoal) -> Self {
        Self { inner: ranged_goal }
    }
}

impl Goal for DrownedTridentAttackGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        if !is_holding_trident(mob) {
            return false;
        }
        self.inner.can_start(mob)
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        if !is_holding_trident(mob) {
            return false;
        }
        self.inner.should_continue(mob)
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.inner.start(mob);
        let trident = mob
            .get_mob_entity()
            .living_entity
            .entity_equipment
            .try_lock()
            .map_or_else(
                |_| ItemStack::new(1, &Item::TRIDENT),
                |eq| eq.get(&EquipmentSlot::MAIN_HAND),
            );

        mob.get_mob_entity().set_attacking(true);
        mob.get_mob_entity().living_entity.set_active_hand(
            pumpkin_util::Hand::Right,
            trident,
            72000,
        );
    }

    fn stop(&mut self, mob: &dyn Mob) {
        self.inner.stop(mob);
        mob.get_mob_entity().living_entity.clear_active_hand();
        mob.get_mob_entity().set_attacking(false);
    }

    fn tick(&mut self, mob: &dyn Mob) {
        self.inner.tick(mob);
    }

    fn controls(&self) -> Controls {
        self.inner.controls()
    }

    fn should_run_every_tick(&self) -> bool {
        self.inner.should_run_every_tick()
    }
}

pub struct DrownedEntity {
    pub entity: Arc<ZombieEntityBase>,
}

impl DrownedEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = ZombieEntityBase::new(entity);
        let drowned = Self { entity };
        let mob_arc = Arc::new(drowned);

        let mob_weak: Weak<dyn RangedAttackMob> = {
            let mob_arc: Arc<dyn RangedAttackMob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.entity.mob_entity.goals_selector;
            let target_selector = &mob_arc.entity.mob_entity.target_selector;

            goal_selector.remove_goal::<ZombieAttackGoal>(&*mob_arc);
            goal_selector.add_goal(
                2,
                Box::new(DrownedTridentAttackGoal::new(RangedAttackGoal::new(
                    mob_weak, 1.0, 40, 10.0,
                ))),
            );
            goal_selector.add_goal(
                3,
                ZombieAttackGoal::new(1.0, false).gated_by(is_not_holding_trident),
            );

            target_selector.add_goal(
                3,
                ActiveTargetGoal::with_default(
                    &mob_arc.entity.mob_entity,
                    &EntityType::AXOLOTL,
                    true,
                ),
            );
        };

        mob_arc
    }

    #[must_use]
    pub fn with_can_break_doors(entity: Entity, can_break_doors: bool) -> Arc<Self> {
        let entity = ZombieEntityBase::with_can_break_doors(entity, can_break_doors);
        let drowned = Self { entity };
        let mob_arc = Arc::new(drowned);

        let mob_weak: Weak<dyn RangedAttackMob> = {
            let mob_arc: Arc<dyn RangedAttackMob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.entity.mob_entity.goals_selector;
            let target_selector = &mob_arc.entity.mob_entity.target_selector;

            goal_selector.remove_goal::<ZombieAttackGoal>(&*mob_arc);
            goal_selector.add_goal(
                2,
                Box::new(DrownedTridentAttackGoal::new(RangedAttackGoal::new(
                    mob_weak, 1.0, 40, 10.0,
                ))),
            );
            goal_selector.add_goal(
                3,
                ZombieAttackGoal::new(1.0, false).gated_by(is_not_holding_trident),
            );

            target_selector.add_goal(
                3,
                ActiveTargetGoal::with_default(
                    &mob_arc.entity.mob_entity,
                    &EntityType::AXOLOTL,
                    true,
                ),
            );
        };

        mob_arc
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.entity.is_baby()
    }

    pub fn set_baby(&self, baby: bool) {
        self.entity.set_baby(baby);
    }
}

impl Mob for DrownedEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn spawn_as_baby(&self) -> bool {
        self.entity.spawn_as_baby()
    }

    fn populate_default_equipment_slots(
        &self,
        _world: &Arc<World>,
        _difficulty: &RegionalDifficulty,
    ) {
        if rand::random::<f32>() < 0.10 {
            let rand = rand::random_range(0..16);
            let item = if rand < 10 {
                &Item::TRIDENT
            } else {
                &Item::FISHING_ROD
            };
            let living = &self.entity.mob_entity.living_entity;
            if let Ok(mut equipment) = living.entity_equipment.lock() {
                equipment.put(&EquipmentSlot::MAIN_HAND, ItemStack::new(1, item));
            }
        }
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.entity.mob_entity.finalize_spawn_base();
        let pos = self.entity.mob_entity.living_entity.entity.pos.load();
        let difficulty = RegionalDifficulty::at(world, pos);
        self.populate_default_equipment_slots(world, &difficulty);
        self.populate_default_equipment_enchantments(&difficulty);

        if rand::random::<f32>() < 0.03 {
            let living = &self.entity.mob_entity.living_entity;
            if let Ok(mut equipment) = living.entity_equipment.lock()
                && equipment.get(&EquipmentSlot::OFF_HAND).is_empty()
            {
                equipment.put(
                    &EquipmentSlot::OFF_HAND,
                    ItemStack::new(1, &Item::NAUTILUS_SHELL),
                );
            }
        }

        group_data
    }

    fn mob_killed_entity(
        &self,
        victim: &dyn EntityBase,
        params: &crate::world::loot::LootContextParameters<'_>,
    ) {
        self.entity.mob_killed_entity(victim, params);
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.entity.mob_write_nbt(nbt);
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.entity.mob_read_nbt(nbt);
    }
}

impl RangedAttackMob for DrownedEntity {
    fn perform_ranged_attack(&self, target: &Arc<dyn EntityBase>, _power: f32) {
        let mob_entity = self.get_mob_entity();
        let entity = &mob_entity.living_entity.entity;
        let world = entity.world.load_full();

        let trident_stack = mob_entity
            .living_entity
            .entity_equipment
            .try_lock()
            .ok()
            .map(|eq| eq.get(&EquipmentSlot::MAIN_HAND))
            .filter(|stack| stack.item.id == Item::TRIDENT.id)
            .unwrap_or_else(|| ItemStack::new(1, &Item::TRIDENT));

        let trident_entity = Entity::new(world.clone(), entity.pos.load(), &EntityType::TRIDENT);
        let trident = TridentEntity::new_shot(
            trident_entity,
            entity,
            trident_stack.clone(),
            ArrowPickup::Disallowed,
        );

        let mob_pos = entity.pos.load();
        let target_entity = target.get_entity();
        let target_pos = target_entity.pos.load();
        let target_dim = target_entity.entity_dimension.load();

        let dx = target_pos.x - mob_pos.x;
        let dy = (target_pos.y + f64::from(target_dim.height) / 3.0) - trident.entity.pos.load().y;
        let dz = target_pos.z - mob_pos.z;
        let horizontal_distance = dx.hypot(dz);

        let difficulty = world.level_info.load().difficulty as i32;
        let divergence = f64::from((14 - difficulty * 4).max(0));

        trident.set_velocity(
            dx,
            horizontal_distance.mul_add(0.2, dy),
            dz,
            1.6,
            divergence,
        );
        trident.apply_on_projectile_spawned(&trident_stack);

        world.spawn_entity(Arc::new(trident));

        let pitch = 1.0 / (rand::random::<f32>() * 0.4 + 0.8);
        world.play_sound_fine(
            Sound::EntityDrownedShoot,
            SoundCategory::Hostile,
            &mob_pos,
            1.0,
            pitch,
        );
    }
}
