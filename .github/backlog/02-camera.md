Caméra de poursuite agréable, inspirée des jeux de kart.

## À faire
- Module `camera/` : la caméra suit derrière le kart du joueur local, avec un lissage par ressort amorti (pas de saccades, pas de mal de mer).
- Elle regarde légèrement devant le kart et anticipe les virages.
- Le FOV s'élargit avec la vitesse et pendant les boosts.
- Léger décalage latéral pendant un drift (préparer le hook, le drift arrive en #4).
- Elle ne passe pas sous le sol (garde-fou simple).
- Paramètres dans une Resource `CameraTuning`.

## Critères d'acceptation
- Aucun tremblement à 30, 60 ou 144 fps : la caméra suit le transform *interpolé*, pas le transform physique.
- Test : la caméra reste dans une distance [min, max] du kart après 5 s de conduite simulée.
