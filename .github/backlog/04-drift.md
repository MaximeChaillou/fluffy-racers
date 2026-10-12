Le drift et les **3 paliers de mini-turbo**, comme dans Mario Kart 8 Deluxe : c'est le cœur du jeu. Toutes les règles et valeurs sont dans `docs/GAME_DESIGN.md` § 3.

## À faire
- Le bouton Saut/Drift déclenche un **saut**. Si on braque, on entre en **drift** à l'atterrissage, du côté du braquage (vitesse minimale requise).
- En drift : angle de glisse maintenu. La direction resserre ou élargit la courbe, sans changer de côté.
- Charge par tick : +5 en braquant vers l'intérieur (≥ 0,5), +2 sinon.
- Paliers 270 / 570 / 870 (modulés plus tard par la stat MT) :
  - étincelles bleues, orange, violettes (provisoires) ;
  - boosts de 0,62 s, 1,67 s et 2,63 s au relâchement.
- Relâcher avant le premier palier ne donne rien.
- Prévoir le cas « direction assistée » : charge réduite et pas de palier violet (les aides arrivent dans une autre issue).
- Valeurs dans `KartTuning`.

## Critères d'acceptation
Tests de scénario :
- drift avec direction intérieure → bleu à 54 ticks, orange à 114, violet à 174 ;
- direction extérieure → environ 2,5 fois plus lent ;
- relâcher à chaque palier → boost de la bonne durée ;
- impossible de drifter à l'arrêt.
