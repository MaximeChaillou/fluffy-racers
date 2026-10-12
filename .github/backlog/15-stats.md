Statistiques des personnages et des karts, sur le modèle de Mario Kart 8 Deluxe. Voir `docs/GAME_DESIGN.md` § 9.

## À faire
- `KartStats` : Vitesse, Accélération, Poids, Maniabilité, Adhérence, Mini-turbo, Invincibilité (terrain « sol » seulement pour l'instant).
- Somme de 4 éléments (personnage + kart + roues + aile), chacun de 0 à 10 points par stat, avec un niveau total borné entre 0 et 20. Affichage `(niveau + 3) / 4`.
- Conversion niveau → paramètres de `KartTuning` par interpolation (vitesse max, accélération, rayon de braquage, seuils et durées de mini-turbo, durée d'invincibilité, poids dans les collisions).
- Données en RON (`assets/data/characters.ron`, `karts.ron`, `wheels.ron`, `wings.ron`) :
  - 9 animaux en 3 gabarits (souris, poussin, hamster / chat, lapin, renard / ours, morse, vache) ;
  - 3 karts, 3 jeux de roues, 2 ailes, avec des compromis clairs.
- Écran de sélection simple : personnage, kart, roues et aile, avec les barres de stats.

## Critères d'acceptation
- Tests : addition et bornes des niveaux.
- Un kart « lourd » a une vitesse max plus haute et une accélération plus basse qu'un « léger ».
- Dans une collision lourd contre léger, c'est le léger qui est le plus repoussé.
