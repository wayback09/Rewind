//! BlockEntityType registry for Minecraft 26.2 (data 4903, proto 776).
//! Derived from `net.minecraft.world.level.block.entity.BlockEntityTypes` order in 26.2.jar
//! (BuiltInRegistries.BLOCK_ENTITY_TYPE). Validated via javap on 26.2.jar.

const NAMES_26_2: &[&str] = &[
    "minecraft:furnace",                 // 0
    "minecraft:chest",                   // 1
    "minecraft:trapped_chest",           // 2
    "minecraft:ender_chest",             // 3
    "minecraft:jukebox",                 // 4
    "minecraft:dispenser",               // 5
    "minecraft:dropper",                 // 6
    "minecraft:sign",                    // 7
    "minecraft:hanging_sign",            // 8
    "minecraft:mob_spawner",             // 9
    "minecraft:creaking_heart",          // 10
    "minecraft:piston",                  // 11
    "minecraft:brewing_stand",           // 12
    "minecraft:enchanting_table",        // 13
    "minecraft:end_portal",              // 14
    "minecraft:beacon",                  // 15
    "minecraft:skull",                   // 16
    "minecraft:daylight_detector",       // 17
    "minecraft:hopper",                  // 18
    "minecraft:comparator",              // 19
    "minecraft:banner",                  // 20
    "minecraft:structure_block",         // 21
    "minecraft:end_gateway",             // 22
    "minecraft:command_block",           // 23
    "minecraft:shulker_box",             // 24
    "minecraft:conduit",                 // 25
    "minecraft:barrel",                  // 26
    "minecraft:smoker",                  // 27
    "minecraft:blast_furnace",           // 28
    "minecraft:lectern",                 // 29
    "minecraft:bell",                    // 30
    "minecraft:jigsaw",                  // 31
    "minecraft:campfire",                // 32
    "minecraft:beehive",                 // 33
    "minecraft:sculk_sensor",            // 34
    "minecraft:calibrated_sculk_sensor", // 35
    "minecraft:sculk_catalyst",          // 36
    "minecraft:sculk_shrieker",          // 37
    "minecraft:chiseled_bookshelf",      // 38
    "minecraft:shelf",                   // 39
    "minecraft:brushable_block",         // 40
    "minecraft:decorated_pot",           // 41
    "minecraft:crafter",                 // 42
    "minecraft:trial_spawner",           // 43
    "minecraft:vault",                   // 44
    "minecraft:test_block",              // 45
    "minecraft:test_instance_block",     // 46
    "minecraft:copper_golem_statue",     // 47
    "minecraft:copper_chest", // 48 - note earlier 48 was copper_chest/pot_sulfur depending on snapshot, this is the 26.2 name
];

/// Return symbolic name for a BlockEntityType numeric id (26.2). None if out of range.
pub fn name_for_id(id: u32) -> Option<&'static str> {
    NAMES_26_2.get(id as usize).copied()
}

/// Number of known types in 26.2.
pub fn len_26_2() -> usize {
    NAMES_26_2.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_known_mappings() {
        assert_eq!(name_for_id(9), Some("minecraft:mob_spawner"));
        assert_eq!(name_for_id(34), Some("minecraft:sculk_sensor"));
        assert_eq!(name_for_id(36), Some("minecraft:sculk_catalyst"));
        assert_eq!(name_for_id(37), Some("minecraft:sculk_shrieker"));
        assert_eq!(name_for_id(1), Some("minecraft:chest"));
        assert_eq!(name_for_id(43), Some("minecraft:trial_spawner"));
        assert_eq!(name_for_id(44), Some("minecraft:vault"));
        assert_eq!(name_for_id(999), None);
    }
}
