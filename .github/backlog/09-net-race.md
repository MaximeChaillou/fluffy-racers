Réseau 3/3 : une vraie course à plusieurs.

## À faire
- Synchronisation d'horloge avec l'hôte (ping/pong et estimation d'offset) pour exprimer les temps en « tick de course ».
- L'hôte choisit le circuit, lance le compte à rebours et fixe le tick de départ : tout le monde part au même moment.
- L'hôte complète la grille avec des bots qu'il simule lui-même et diffuse comme ses propres karts.
- Passages de checkpoints et arrivées envoyés en événements fiables ; classement partagé et écran de résultats commun.
- Migration d'hôte : si l'hôte part, le plus petit `PeerId` restant reprend l'arbitrage.

## Critères d'acceptation
- Une course de 3 tours à 2 joueurs + bots est jouable de bout en bout en local avec `matchbox_server`.
- Tests unitaires de l'estimation d'offset d'horloge.
