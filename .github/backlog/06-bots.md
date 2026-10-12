Des bots pour remplir la grille, et un test de charge à 12 karts.

## À faire
- Module `bots/` : un bot produit un `KartInput` (exactement comme un joueur) en suivant la ligne de course du circuit (#3).
- Niveau de difficulté réglable : précision de la trajectoire, anticipation des virages, usage du drift.
- Évitement basique des autres karts et un peu de variété pour qu'ils ne roulent pas en file indienne.
- Grille de 12 : 1 joueur et 11 bots.
- Overlay de debug (touche F3) : fps, temps de frame, nombre de karts.

## Critères d'acceptation
- Test headless : 11 bots terminent 3 tours sans rester bloqués (un timeout fait échouer le test).
- Noter dans `STATUS.md` le temps de frame mesuré avec 12 karts (natif et, si possible, web).
