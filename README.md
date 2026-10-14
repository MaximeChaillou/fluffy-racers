# Fluffy Racers

Jeu de kart multijoueur dans le navigateur, avec des animaux tout mous. Rust + Bevy, compilé en WebAssembly.

**Jouer :** https://maximechaillou.github.io/fluffy-racers/ (la dernière version de `dev`, mise à jour automatiquement)

Branches : `dev` reçoit automatiquement le travail de Claude (fusionné quand la CI passe), `main` est la version stable.

## Développer

Prérequis : Rust stable (installé via [rustup](https://rustup.rs)) et [trunk](https://trunkrs.dev).

```bash
cargo run      # version native
trunk serve    # version web sur http://localhost:8080
cargo test
```

Sur Linux, il faut aussi : `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

## Jouer à plusieurs (dev)

Les joueurs se connectent directement entre eux (WebRTC, pair à pair). Pour se trouver, ils passent par un petit **serveur de mise en relation** : il n'échange que quelques Ko au moment de rejoindre, aucune donnée de jeu n'y passe. En attendant celui de production (issue #10), on lance `matchbox_server` sur sa machine.

Pour l'instant (réseau 1/3, issue #7), on se retrouve dans le **salon** (touche **L**) : la liste des joueurs, le ping de chacun, l'hôte et le bouton « Prêt ». La course reste en solo : les karts des autres arrivent avec #8, la course synchronisée avec #9.

### 1. Installer et lancer le serveur de mise en relation

```bash
cargo install matchbox_server --version 0.14.0   # une seule fois (environ 2 min)
matchbox_server                                  # écoute sur le port 3536 ; laisse-le tourner
```

### 2. Deux onglets

```bash
trunk serve          # dans un autre terminal : le jeu sur http://localhost:8080
```

1. Ouvre http://localhost:8080, appuie sur **L**, puis clique sur **Créer une partie** : l'adresse devient `http://localhost:8080/?room=CODE`, et le salon montre le lien d'invitation (bouton **Copier le lien**).
2. Colle ce lien dans un **deuxième onglet, ouvert dans une nouvelle fenêtre**, et mets les deux fenêtres côte à côte : le navigateur ralentit beaucoup un onglet caché (le jeu y tourne au ralenti, son ping monte et il peut s'afficher « muet »).
3. Après quelques secondes, chaque fenêtre montre les deux joueurs : le nom (celui de l'animal, ou `&pseudo=Max` dans l'adresse), le ping en millisecondes, « hôte » sur l'un des deux.
4. Clique sur **Prêt !** dans une fenêtre : « prêt » s'affiche aussi dans l'autre.
5. Ferme un onglet : il disparaît du salon de l'autre (tout de suite avec **Quitter la partie**). Si c'était l'hôte, l'autre le devient.

Si les joueurs restent seuls chacun de leur côté (« 1 joueur(s) sur 12 » dans chaque fenêtre), la connexion directe ne s'est pas faite : regarde la console du navigateur (F12) et signale-le.

### 3. Deux navigateurs

Même chose avec le lien ouvert dans un autre navigateur (Chrome et Firefox, par exemple), sur la même machine.

### 4. Deux machines

Il faut que les deux machines joignent le même serveur de mise en relation :

- sur le même réseau local : lance `matchbox_server` sur la machine A, puis ouvre le jeu sur les deux machines avec `?room=CODE&signal=ws://IP_DE_A:3536` (`trunk serve --address 0.0.0.0` pour servir le jeu aux autres machines, à l'adresse `http://IP_DE_A:8080`) ;
- par Internet : il faut un serveur de mise en relation public en `wss://` (issue #10, en attente) ; `?signal=wss://…` le désigne.

La connexion directe entre les deux machines passe par des serveurs STUN publics (Google). Sans serveur TURN, certains réseaux (entreprise, 4G) peuvent la bloquer.

### Sur la version en ligne (GitHub Pages)

La page est en `https`, et le serveur par défaut est `ws://localhost:3536` (non chiffré). Chrome l'accepte quand même pour `localhost` ; Firefox aussi en principe ; Safari le refuse sans doute (contenu mixte). Le plus sûr pour tester : `trunk serve` en local, comme plus haut.

### En natif

```bash
cargo run -- --room CODE                 # rejoint la partie CODE (L ouvre le salon)
cargo run -- --room CODE --pseudo Max --signal ws://192.168.1.20:3536
```

Un test automatique fait se rencontrer deux clients natifs (serveur lancé) :

```bash
cargo test --lib net::scenario_tests::two_native_peers -- --ignored --nocapture
```

## Comment ce projet est construit

Le code est écrit par Claude Code, la plupart du temps en tâche de fond :

- [`CLAUDE.md`](CLAUDE.md) : règles, stack figée, conventions ;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) : kart, réseau P2P, personnages ;
- [`STATUS.md`](STATUS.md) : où on en est, décisions, questions ouvertes ;
- les **issues GitHub** forment le backlog : une issue = une PR.

## Crédits

Voir [`assets/CREDITS.md`](assets/CREDITS.md).
