Des bots pour remplir la grille de 12, et un test de charge. Voir `docs/GAME_DESIGN.md` § 12.

## À faire
- Module `bots/` : un bot produit un `KartInput`, exactement comme un joueur, en suivant la ligne de course du circuit.
- Trois niveaux de difficulté : précision de trajectoire, drift (palier visé : bleu, orange ou violet), anticipation des virages.
- Léger rattrapage (≤ 3 % de vitesse max, réglable).
- Évitement basique et variété, pour éviter la file indienne.
- Grille de 12 : 1 joueur et 11 bots.
- Overlay de debug (F3) : fps, temps de frame, nombre de karts.
- L'usage des objets par les bots viendra avec le système d'objets.

## Critères d'acceptation
- Test headless : 11 bots terminent 3 tours sans rester bloqués.
- En difficulté max, les bots enchaînent des mini-turbos orange.
- Temps de frame avec 12 karts noté dans `STATUS.md`.
