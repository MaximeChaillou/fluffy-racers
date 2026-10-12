Premier jalon : un kart agréable à conduire, avec le ressenti de Mario Kart 8 Deluxe comme référence. Voir `docs/GAME_DESIGN.md` § 2 et `docs/ARCHITECTURE.md` § « Le kart ».

## À faire
- Ajouter `bevy_rapier3d` 0.37.
- Module `kart/` : `KartInput`, `KartTuning` (Resource documentée, valeurs par défaut du game design) et une **machine à états** (`Grounded`, `Airborne`… ; le drift viendra plus tard).
- Physique arcade :
  - suspension par 4 raycasts ;
  - courbe moteur vers la vitesse max (150cc : 26 m/s) ;
  - grip latéral fort ;
  - direction pilotée en lacet selon la vitesse ;
  - gravité renforcée et redressement automatique.
- Frein et marche arrière sur le même bouton.
- **Hors-piste** : vitesse max réduite (environ 55 %) sur les surfaces marquées.
- Composant `Boost { strength, remaining }` générique, testé avec une touche de debug.
- Entrées clavier et manette selon le tableau du game design (§ 13). Le kart ne lit que `KartInput`.
- Simulation en `FixedUpdate` 60 Hz, interpolation visuelle dans `Update`.
- Terrain de test : sol plat, une zone d'herbe, des obstacles, une rampe.
- Remplacer le `PlaceholderKart`.

## Critères d'acceptation
Tests de scénario headless :
- accélérer à fond 2,5 s → au moins 95 % de la vitesse max ;
- en herbe → la vitesse plafonne autour de 55 % ;
- un boost dépasse la vitesse max, puis on y redescend ;
- braquer 3 s à pleine vitesse → le kart ne se retourne pas ;
- après le saut de rampe → il retombe sur ses roues.
