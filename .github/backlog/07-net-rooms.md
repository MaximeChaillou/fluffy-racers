Réseau 1/3 : se connecter entre navigateurs dans une room. Voir `docs/ARCHITECTURE.md` § « Réseau ».

## À faire
- Ajouter `matchbox_socket` 0.14, à utiliser directement (pas `bevy_matchbox`, incompatible avec Bevy 0.20). Le poller depuis un système Bevy, en natif comme en wasm.
- Room via l'URL `?room=CODE` sur le web, ou via un argument CLI en natif. Un bouton « Créer une partie » génère un code et un lien à partager.
- URL de signalisation configurable : `ws://localhost:3536` par défaut en dev.
- Deux canaux : non fiable (états) et fiable (événements).
- Lobby simple : liste des joueurs, ping de chacun, hôte = plus petit `PeerId`, bouton « Prêt ».
- Gestion propre des arrivées et des départs.
- Ajouter au README comment tester avec `matchbox_server` et deux onglets.

## Critères d'acceptation
- Tests de sérialisation des messages (aller-retour).
- La PR explique pas à pas comment Max teste avec 2 onglets et 2 navigateurs.
