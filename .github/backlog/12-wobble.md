Le côté « animal tout mou » : un effet procédural, purement visuel. Voir `docs/ARCHITECTURE.md` § « Personnages ».

## À faire
- Module `characters/` : ressorts amortis sur la tête, les oreilles et la queue, qui suivent le kart avec un léger retard.
- Squash & stretch aux atterrissages et aux chocs.
- Touché ou gros choc → mini-ragdoll de la tête et des bras pendant environ 1 s, puis retour en place.
- D'abord sur le pilote en primitives (sphères), compatible plus tard avec les modèles de #11.
- Paramètres dans une Resource ; ne touche jamais la physique du kart, rien n'est synchronisé en réseau.

## Critères d'acceptation
- Test : après une impulsion, le ressort revient à l'équilibre en moins de N secondes, sans diverger.
- La PR décrit ce que Max doit observer (atterrissage après la rampe, choc contre un mur).
