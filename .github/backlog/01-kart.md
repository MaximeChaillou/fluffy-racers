Premier jalon : un kart agréable à conduire sur un terrain plat. Voir `docs/ARCHITECTURE.md` § « Le kart ».

## À faire
- Ajouter `bevy_rapier3d` 0.37 (seule lib physique compatible Bevy 0.20).
- Module `kart/` : `KartInput`, `KartTuning` (Resource documentée) et physique arcade :
  - corps rigide dynamique ;
  - suspension par 4 raycasts ;
  - moteur, grip latéral, direction pilotée en lacet selon la vitesse ;
  - gravité renforcée et redressement automatique.
  - **Pas de drift** pour l'instant (#4).
- Entrées : clavier (flèches et WASD/ZQSD ; Espace réservé au drift, Shift aux objets) et manette. Le kart ne lit que `KartInput`.
- Simulation en `FixedUpdate` 60 Hz, avec interpolation visuelle du transform dans `Update`.
- Terrain de test : sol plat, quelques obstacles, une rampe.
- Remplacer le `PlaceholderKart` qui tourne sur lui-même.

## Critères d'acceptation
- Tests de scénario headless :
  - accélérer à fond pendant 2 s → la vitesse est dans une plage attendue ;
  - relâcher → le kart décélère jusqu'à l'arrêt ;
  - braquer à pleine vitesse pendant 3 s → le kart tourne sans se retourner ;
  - sauter de la rampe → il retombe sur ses roues.
- La PR liste les valeurs de `KartTuning` et explique comment les ajuster.
