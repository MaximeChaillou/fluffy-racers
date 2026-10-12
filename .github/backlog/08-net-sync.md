Réseau 2/3 : voir les autres karts rouler de façon fluide.

## À faire
- Chaque pair diffuse l'état de **son** kart à 30 Hz, quantifié et compact (< 64 octets) : tick, position, rotation, vitesses, flags (drift, boost, touché).
- Karts distants : buffer d'interpolation d'environ 100 ms, extrapolation courte en cas de trou ; corps **kinématiques** en local.
- Collisions kart contre kart : chaque pair applique le choc sur son propre kart.
- Mode debug pour simuler la latence, la gigue et la perte de paquets (ex. 120 ms, 5 % de perte).
- Jusqu'à 12 pairs.

## Critères d'acceptation
- Tests : aller-retour de la sérialisation, taille du message, interpolation correcte entre deux snapshots, extrapolation bornée.
- Avec la simulation à 120 ms et 5 % de perte, les karts distants restent fluides (à décrire dans la PR).
