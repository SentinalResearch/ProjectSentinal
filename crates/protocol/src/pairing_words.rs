//! The fixed verification word list (4.E §7): 256 short, distinct English
//! words, one per byte value. Changing it changes every derived word, so it
//! is part of the `sentinal-pair-v1` contract.

/// One word per byte value.
pub const WORDS: [&str; 256] = [
    "acorn", "amber", "anchor", "apple", "arrow", "aspen", "atlas", "autumn", "badge", "bagel",
    "bamboo", "banjo", "barley", "basil", "beacon", "bear", "berry", "birch", "bison", "blanket",
    "bloom", "bolt", "bonsai", "border", "breeze", "brick", "bridge", "brook", "bubble", "bucket",
    "buffalo", "bugle", "cabin", "cactus", "camel", "candle", "canoe", "canyon", "carbon",
    "carrot", "castle", "cedar", "cello", "chalk", "cherry", "chess", "cider", "cinder", "circle",
    "clover", "cobalt", "cocoa", "comet", "copper", "coral", "cotton", "cougar", "crane", "crater",
    "cricket", "crystal", "cumin", "cypress", "daisy", "delta", "denim", "desert", "diesel",
    "dingo", "dolphin", "donkey", "dragon", "drum", "eagle", "easel", "ebony", "echo", "eclipse",
    "elbow", "elder", "ember", "emerald", "engine", "falcon", "fennel", "fern", "ferry", "fiddle",
    "fig", "flame", "flint", "flute", "forest", "fossil", "fox", "galaxy", "garden", "garnet",
    "gecko", "geyser", "ginger", "glacier", "globe", "goose", "granite", "grape", "gravel",
    "guitar", "gull", "hammer", "harbor", "harp", "hazel", "heron", "hickory", "honey", "hornet",
    "husky", "igloo", "indigo", "iris", "island", "ivory", "jacket", "jade", "jasmine", "jelly",
    "jungle", "juniper", "kayak", "kettle", "kiwi", "koala", "ladder", "lagoon", "lantern", "lava",
    "lemon", "lilac", "lime", "linen", "lizard", "llama", "lobster", "lotus", "lumen", "lunar",
    "magnet", "mango", "maple", "marble", "meadow", "melon", "meteor", "mint", "mitten", "mocha",
    "monsoon", "moose", "mosaic", "nectar", "needle", "nickel", "noodle", "nutmeg", "oasis",
    "ocean", "olive", "onyx", "orbit", "orchid", "otter", "owl", "oyster", "paddle", "panda",
    "papaya", "parrot", "peach", "pebble", "pecan", "pepper", "piano", "pigeon", "pine", "pixel",
    "planet", "plum", "polar", "pony", "poppy", "prism", "puffin", "pumpkin", "quail", "quartz",
    "quill", "rabbit", "radar", "radish", "raven", "reef", "ribbon", "river", "robin", "rocket",
    "ruby", "saddle", "saffron", "salmon", "sandal", "satin", "seal", "sequoia", "shadow", "shell",
    "sierra", "silver", "sketch", "sloth", "snail", "sonnet", "sparrow", "spice", "sprout",
    "squid", "summit", "sunset", "swan", "tango", "teapot", "thistle", "thunder", "tiger",
    "timber", "toast", "topaz", "tornado", "tulip", "tundra", "turnip", "turtle", "tuxedo",
    "umber", "valley", "velvet", "violet", "violin", "walnut", "walrus", "willow", "window",
    "yacht", "yarrow", "zebra", "zephyr",
];
