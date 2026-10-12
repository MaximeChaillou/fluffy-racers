Réseau 3/3 : une vraie course à plusieurs, avec objets et croquettes.

## À faire
- Synchronisation d'horloge avec l'hôte (ping/pong et offset) pour exprimer les temps en « tick de course ».
- L'hôte choisit le circuit, lance le compte à rebours et fixe le tick de départ. Le départ fusée de chacun est calculé localement.
- L'hôte complète la grille avec des bots qu'il simule et diffuse comme ses propres karts.
- Croquettes et boîtes à objets : l'hôte arbitre les ramassages (événements fiables). La roulette est tirée localement.
- Objets : le lanceur fait autorité sur la trajectoire, la victime décide si elle est touchée (voir `docs/ARCHITECTURE.md`).
- Checkpoints et arrivées en événements fiables, classement partagé, résultats communs.
- Migration d'hôte : le plus petit `PeerId` restant reprend l'arbitrage.

## Critères d'acceptation
- Une course de 3 tours à 2 joueurs + bots, avec objets, est jouable de bout en bout en local avec `matchbox_server`.
- Tests unitaires de l'estimation d'offset d'horloge.
