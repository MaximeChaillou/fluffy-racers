Les techniques de pilotage de Mario Kart 8 Deluxe : départ fusée, figures, aspiration, panneaux. Voir `docs/GAME_DESIGN.md` § 2, 4, 5 et 6.

## À faire
- **Départ fusée** : accélérer juste après l'apparition du « 2 ».
  - Boost maximal de 2,5 s si l'appui arrive dans les 0,25 s, puis durée décroissante jusqu'à 0,5 s à 1 s.
  - Appui trop tôt → calage de 1 s.
  - Flammes proportionnelles à la qualité du départ.
- **Figures** : appuyer sur Saut/Drift en quittant un tremplin marqué (fenêtre d'environ 0,2 s) → animation de figure, puis boost de 0,5 à 0,8 s à l'atterrissage. Les tremplins sont marqués dans les données du circuit.
- **Aspiration** : à moins de 5 m derrière un kart, dans un cône de ±12° et au-dessus de 60 % de la vitesse max, une jauge se charge. Au bout de 1,2 s → boost de 1 s. Lignes de vent provisoires.
- **Panneaux d'accélération** sur la piste : boost d'environ 1 s.
- Toutes les valeurs dans `KartTuning`.

## Critères d'acceptation
Tests de scénario :
- appui à t = 0,1 s, 0,6 s et 1,2 s après le « 2 » → boost de 2,5 s, d'environ 1,6 s, puis aucun ;
- appui avant le « 2 » → calage ;
- aspiration derrière un bot à vitesse constante → boost après 1,2 s ;
- figure ratée (pas d'appui) → pas de boost.
