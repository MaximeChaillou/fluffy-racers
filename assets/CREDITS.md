# Crédits des assets

Chaque asset ajouté au jeu a une ligne ici. Licences acceptées : CC0, CC BY (avec attribution ci-dessous).

Modèles 3D (#11) : une ligne par pack, avec chacun de ses fichiers. Seuls les GLB utilisés sont dans le dépôt (et la texture `colormap.png` des packs qui en ont une) ; la licence du pack est à côté d'eux. Un test (`models::tests`) vérifie que chaque modèle nommé par le jeu est là, avec sa ligne ici, et qu'aucun autre fichier n'est dans `assets/models/`.

| Fichier(s) | Source | Auteur | Licence |
|---|---|---|---|
| `models/car-kit/` (1 modèle + `Textures/colormap.png`, licence dans `License.txt`) : kart-oobi.glb | [Car Kit](https://kenney.nl/assets/car-kit) 3.1 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/nature-kit/` (19 modèles, licence dans `License.txt`) : flower_purpleA.glb, flower_redA.glb, flower_yellowA.glb, mushroom_redGroup.glb, mushroom_tanGroup.glb, plant_bush.glb, plant_bushDetailed.glb, plant_bushLarge.glb, rock_largeA.glb, rock_largeB.glb, rock_tallA.glb, stump_round.glb, tree_cone.glb, tree_default.glb, tree_detailed.glb, tree_fat.glb, tree_oak.glb, tree_pineDefaultA.glb, tree_pineRoundA.glb | [Nature Kit](https://kenney.nl/assets/nature-kit) 2.1 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/racing-kit/` (8 modèles, licence dans `License.txt`) : bannerTowerGreen.glb, bannerTowerRed.glb, flagCheckers.glb, flagGreen.glb, flagRed.glb, grandStandAwning.glb, grandStandCovered.glb, tentClosed.glb | [Racing Kit](https://kenney.nl/assets/racing-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/food-kit/` (12 modèles + `Textures/colormap.png`, licence dans `License.txt`) : cake-birthday.glb, cherries.glb, cookie-chocolate.glb, cupcake.glb, donut-chocolate.glb, donut-sprinkles.glb, lollypop.glb, muffin.glb, popsicle.glb, strawberry.glb, sundae.glb, whipped-cream.glb | [Food Kit](https://kenney.nl/assets/food-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/holiday-kit/` (5 modèles + `Textures/colormap.png`, licence dans `License.txt`) : candy-cane-green.glb, candy-cane-red.glb, gingerbread-man.glb, present-a-cube.glb, present-b-round.glb | [Holiday Kit](https://kenney.nl/assets/holiday-kit) 2.0 | Kenney (www.kenney.nl) | CC0 1.0 |
| `models/cube-pets/` (9 modèles + `Textures/colormap.png`, licence dans `License.txt`) : animal-chick.glb, animal-penguin.glb, animal-parrot.glb, animal-cat.glb, animal-bunny.glb, animal-fox.glb, animal-pig.glb, animal-cow.glb, animal-elephant.glb (les pilotes, #69) | [Cube Pets](https://kenney.nl/assets/cube-pets) 1.0 | Kenney (www.kenney.nl) | CC0 1.0 |

## Polices embarquées par une crate (pas des fichiers du dépôt)

Le panneau d'admin (#36) s'affiche avec les polices par défaut d'egui, compilées dans le jeu par la crate `epaint_default_fonts` (feature `default_fonts` de `bevy_egui`). Elles ne servent qu'au panneau.

| Police | Source | Auteur | Licence |
|---|---|---|---|
| Ubuntu Light | crate `epaint_default_fonts` 0.36 | Dalton Maag / Canonical | Ubuntu Font Licence 1.0 |
| Hack Regular | crate `epaint_default_fonts` 0.36 | Source Foundry | MIT + Bitstream Vera |
| Noto Emoji Regular | crate `epaint_default_fonts` 0.36 | Google | SIL OFL 1.1 |
| emoji-icon-font | crate `epaint_default_fonts` 0.36 | John Slegers | MIT |
