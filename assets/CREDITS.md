# Crédits des assets

Chaque asset ajouté au jeu a une ligne ici. Licences acceptées : CC0, CC BY (avec attribution ci-dessous).

Modèles 3D (#11) : une ligne par pack, avec chacun de ses fichiers. Le panier en osier, la baignoire, les bobines de fil et la plume (#73) ne sont pas des fichiers : ils sont construits par le jeu (`src/kart/parts.rs`), aucun pack libre n'en ayant. Seuls les GLB utilisés sont dans le dépôt (et la texture `colormap.png` des packs qui en ont une) ; la licence du pack est à côté d'eux. Un test (`models::tests`) vérifie que chaque modèle nommé par le jeu est là, avec sa ligne ici, et qu'aucun autre fichier n'est dans `assets/models/`.

| Fichier(s) | Source | Auteur | Licence |
|---|---|---|---|
| `models/car-kit/` (2 modèles + `Textures/colormap.png`, licence dans `License.txt`) : kart-oobi.glb (le kart, la caisse à savon, et ses roues, les pneus guimauve), wheel-tractor-back.glb (les pneus de tracteur, #73) | [Car Kit](https://kenney.nl/assets/car-kit) 3.1 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/nature-kit/` (41 modèles, licence dans `License.txt`) : flower_purpleA.glb, flower_redA.glb, flower_redB.glb, flower_yellowA.glb, flower_yellowB.glb, grass_large.glb, grass_leafs.glb, mushroom_redGroup.glb, mushroom_tanGroup.glb, plant_bush.glb, plant_bushDetailed.glb, plant_bushLarge.glb, plant_bushSmall.glb, plant_flatShort.glb, plant_flatTall.glb, rock_largeA.glb, rock_largeB.glb, rock_tallA.glb, rock_tallB.glb, rock_tallC.glb, statue_column.glb, statue_head.glb, statue_obelisk.glb, stone_tallA.glb, stone_tallB.glb, stump_round.glb, tree_cone.glb, tree_default.glb, tree_detailed.glb, tree_fat.glb, tree_oak.glb, tree_palm.glb, tree_palmBend.glb, tree_palmDetailedShort.glb, tree_palmDetailedTall.glb, tree_palmShort.glb, tree_palmTall.glb, tree_pineDefaultA.glb, tree_pineRoundA.glb, tree_pineTallA.glb, tree_pineTallB.glb (le Lagon Duvet, #20 : palmiers, rochers, statues, plantes) | [Nature Kit](https://kenney.nl/assets/nature-kit) 2.1 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/racing-kit/` (8 modèles, licence dans `License.txt`) : bannerTowerGreen.glb, bannerTowerRed.glb, flagCheckers.glb, flagGreen.glb, flagRed.glb, grandStandAwning.glb, grandStandCovered.glb, tentClosed.glb | [Racing Kit](https://kenney.nl/assets/racing-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/food-kit/` (12 modèles + `Textures/colormap.png`, licence dans `License.txt`) : cake-birthday.glb, cherries.glb, cookie-chocolate.glb, cupcake.glb, donut-chocolate.glb, donut-sprinkles.glb, lollypop.glb, muffin.glb, popsicle.glb, strawberry.glb, sundae.glb, whipped-cream.glb | [Food Kit](https://kenney.nl/assets/food-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/holiday-kit/` (18 modèles + `Textures/colormap.png`, licence dans `License.txt`) : bench.glb, candy-cane-green.glb, candy-cane-red.glb, gingerbread-man.glb, lantern.glb, nutcracker.glb, present-a-cube.glb, present-b-cube.glb, present-b-round.glb, reindeer.glb, sled.glb, snow-pile.glb, snowman-hat.glb, snowman.glb, tree-decorated-snow.glb, tree-snow-a.glb, tree-snow-b.glb, tree-snow-c.glb | [Holiday Kit](https://kenney.nl/assets/holiday-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/cube-pets/` (9 modèles + `Textures/colormap.png`, licence dans `License.txt`) : animal-chick.glb, animal-penguin.glb, animal-parrot.glb, animal-cat.glb, animal-bunny.glb, animal-fox.glb, animal-pig.glb, animal-cow.glb, animal-elephant.glb (les pilotes, #69) | [Cube Pets](https://kenney.nl/assets/cube-pets) 1.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/pirate-kit/` (22 modèles + `Textures/colormap.png`, licence dans `License.txt`) : barrel.glb, boat-row-small.glb, cannon.glb, chest.glb, crate-bottles.glb, crate.glb, flag-pirate.glb, flag.glb, grass-plant.glb, palm-bend.glb, palm-detailed-bend.glb, palm-detailed-straight.glb, palm-straight.glb, rocks-a.glb, rocks-b.glb, rocks-c.glb, rocks-sand-a.glb, rocks-sand-b.glb, rocks-sand-c.glb, ship-pirate-small.glb, ship-wreck.glb, tower-complete-small.glb (la Crique Câline, #19 ; le Lagon Duvet, #20) | [Pirate Kit](https://kenney.nl/assets/pirate-kit) 2.1 | Kenney (www.kenney.nl) | CC0 1.0 |

## Interface (#56)

Les écrans de jeu (le menu principal, puis ceux de l'étape 2) : une police libre et des icônes CC0, dans `assets/ui/`. Seuls les fichiers utilisés sont dans le dépôt, la licence de chaque pack à côté d'eux. Un test (`menu::style::tests`) vérifie que chaque fichier nommé par le jeu est là, avec sa ligne ici, et qu'aucun autre fichier n'est dans `assets/ui/`.

| Fichier(s) | Source | Auteur | Licence |
|---|---|---|---|
| `ui/fonts/` (2 polices, licence dans `OFL.txt`) : Fredoka-Medium.ttf, Fredoka-SemiBold.ttf (les graisses 500 et 600, instances statiques servies par Google Fonts) | [Fredoka](https://fonts.google.com/specimen/Fredoka) 2.001 | The Fredoka Project Authors (Milena Brandão, Hafontia) | SIL OFL 1.1 |
| `ui/icons/` (6 icônes blanches de 100 px, licence dans `License.txt`) : right.png, singleplayer.png, multiplayer.png, gear.png, wrench.png (le menu principal), home.png (« Quitter », retour à l'accueil, #89) | [Game Icons](https://kenney.nl/assets/game-icons) (2014) | Kenney (www.kenney.nl) | CC0 1.0 |

## Polices embarquées par une crate (pas des fichiers du dépôt)

Le panneau d'admin (#36) s'affiche avec les polices par défaut d'egui, compilées dans le jeu par la crate `epaint_default_fonts` (feature `default_fonts` de `bevy_egui`). Elles ne servent qu'au panneau.

| Police | Source | Auteur | Licence |
|---|---|---|---|
| Ubuntu Light | crate `epaint_default_fonts` 0.36 | Dalton Maag / Canonical | Ubuntu Font Licence 1.0 |
| Hack Regular | crate `epaint_default_fonts` 0.36 | Source Foundry | MIT + Bitstream Vera |
| Noto Emoji Regular | crate `epaint_default_fonts` 0.36 | Google | SIL OFL 1.1 |
| emoji-icon-font | crate `epaint_default_fonts` 0.36 | John Slegers | MIT |
