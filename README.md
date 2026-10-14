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

### 2. Deux fenêtres sur ton ordinateur

```bash
trunk serve          # dans un autre terminal : le jeu sur http://localhost:8080
```

1. Dans Chrome, ouvre http://localhost:8080/?room=TEST : le salon s'ouvre tout seul (sinon, **L**). Ou bien appuie sur **L** puis **Créer une partie** : l'adresse devient `http://localhost:8080/?room=CODE`, et le salon montre le lien d'invitation (bouton **Copier le lien**).
2. Ouvre la même adresse dans une **nouvelle fenêtre** Chrome, et mets les deux fenêtres côte à côte : le navigateur ralentit beaucoup un onglet caché (le jeu y tourne au ralenti, son ping monte et il peut s'afficher « muet »).
3. En une ou deux secondes, chaque fenêtre montre les deux joueurs : le nom (celui de l'animal, ou `&pseudo=Max` dans l'adresse), le ping en millisecondes, « hôte » sur l'un des deux. En bas du salon : « Connexion directe : sans serveur STUN » (inutile en local).
4. Dans le terminal de `matchbox_server` : deux lignes `Client connected …`, puis une ligne `… -> "{\"NewPeer\":…}"` (le serveur présente le second au premier). **Le serveur n'écrit jamais les offres ni les réponses** (`Offer`, `Answer`, `IceCandidate`), même quand tout marche : c'est la console du navigateur qui les montre.
5. Dans la console du navigateur (F12, onglet Console), les étapes de la connexion : dans la première fenêtre, `network: #f599 announced by the signaling server: we make the offer`, `network: offer sent to #f599 after 0.01 s: 1 candidate (1 host .local)`, `network: answer received from #f599…` ; dans l'autre, `network: offer received…` et `network: answer sent…` ; puis dans les deux `network: #… connected, 0.05 s after it was announced…`.
6. Clique sur **Prêt !** dans une fenêtre : « prêt » s'affiche aussi dans l'autre. Ferme un onglet : il disparaît du salon de l'autre (tout de suite avec **Quitter la partie**). Si c'était l'hôte, l'autre le devient.
7. Même chose avec **Chrome + Safari** : ouvre la même adresse dans Safari. Pour voir sa console : Safari > Réglages > Avancés > « Afficher les fonctionnalités pour les développeurs web », puis Développement > Afficher la console JavaScript.

Si la connexion ne se fait pas, le salon le dit (« connexion directe en cours… », puis « n'aboutit pas » avec les pistes) : voir **Dépannage** plus bas.

### 3. Jouer sur le réseau local

Deux appareils sur le même Wi-Fi (ton Mac et un autre ordinateur, par exemple), ou deux navigateurs de ton Mac via son adresse sur le réseau.

1. Trouve l'adresse IP de ton Mac : `ipconfig getifaddr en0` (Wi-Fi ; si rien ne s'affiche, essaie `en1`), par exemple `192.168.1.20`.
2. Lance `matchbox_server`, puis `trunk serve` : le jeu écoute sur toutes les interfaces (`Trunk.toml`), et trunk affiche ses adresses au démarrage.
3. Sur les deux appareils, ouvre `http://192.168.1.20:8080/?room=TEST` (avec ton IP). La page trouve toute seule le serveur de mise en relation sur la même machine (`ws://192.168.1.20:3536`, affiché dans le salon), sans `&signal=`. Le lien d'invitation du salon reprend cette adresse.
4. Si macOS demande d'**accepter les connexions entrantes** (pour `matchbox_server` ou `trunk`) ou si le navigateur veut **accéder aux appareils du réseau local**, accepte.
5. Ce que tu dois voir : comme plus haut, les deux joueurs avec leur ping ; dans la console, `offer sent` / `answer received` puis `connected`. En bas du salon : « Mise en relation : ws://192.168.1.20:3536 » et « Connexion directe : sans serveur STUN ».

Ouverte en `localhost`, la page ne connaît pas l'adresse de ton Mac sur le réseau (le salon le rappelle) : pour inviter quelqu'un, ouvre-la avec l'adresse IP.

### 4. Par Internet

Il faut un serveur de mise en relation public en `wss://` (issue #10, en attente) ; `?signal=wss://…` le désigne. Le lien d'invitation garde ce paramètre.

Par Internet, chaque navigateur demande son adresse publique aux serveurs STUN publics de Google. Sans relais TURN, certains réseaux (entreprise, 4G) bloquent la connexion directe ; un relais se donne avec `?turn=turn:relais.exemple.org:3478&turnuser=NOM&turnpass=MOT_DE_PASSE` (celui de production viendra avec #10).

### Réglages de la connexion

| Page | Natif | Effet |
|---|---|---|
| `?signal=ws://…` | `--signal ws://…` | Le serveur de mise en relation. Par défaut : celui de la machine qui sert la page quand elle est locale, sinon `ws://localhost:3536`. |
| `?stun=none` | `--stun none` | Aucun serveur STUN (le défaut quand la mise en relation est locale). |
| `?stun=stun:hôte:port,…` | `--stun …` | Ces serveurs STUN. `?stun=auto` : le défaut. |
| `?turn=turn:hôte:port&turnuser=…&turnpass=…` | `--turn … --turnuser … --turnpass …` | Un relais TURN (les trois sont nécessaires). Aucun par défaut. |

Une valeur illisible est ignorée, avec un avertissement dans la console.

### Dépannage

**Le salon dit « la connexion directe … n'aboutit pas »**, ou chacun reste seul (« 1 joueur(s) sur 12 »). Pistes, dans l'ordre :

1. **macOS** : Réglages Système > Confidentialité et sécurité > **Réseau local** : active ton navigateur (Chrome, Safari…), puis quitte-le et relance-le. Sans cette autorisation, le navigateur ne peut pas annoncer ses adresses locales (mDNS).
2. **Chrome** : ouvre `chrome://flags/#enable-webrtc-hide-local-ips-with-mdns` (« Anonymize local IPs exposed by WebRTC »), passe-le à **Disabled**, puis relance Chrome. C'est ce qui a débloqué Max (#49) : Chrome cache ses adresses locales derrière des noms `….local`, et sur son Mac ces noms ne marchaient pas.
3. **Un réseau qui bloque l'UDP** (entreprise) : il faudra un relais TURN (`?turn=…`, prévu avec #10).

Autres causes possibles : une extension anti-fuite WebRTC (uBlock Origin « Empêcher WebRTC de divulguer les adresses IP locales », WebRTC Leak Prevent…), un VPN, une politique Chrome (`chrome://policy`, `WebRtcIPHandlingPolicy`). Pour les écarter : essaie en navigation privée (sans extensions).

**La console** (F12) écrit chaque étape : `announced`, `offer sent … N candidates`, `answer received …`, `connected`. `no candidate` dans une offre ou une réponse : ce navigateur n'a trouvé aucune adresse où l'autre peut le joindre (pistes 1 et 2). `[fluffy-net] the direct connection failed (ICE failed)` : les deux navigateurs ont essayé toutes leurs adresses, sans réponse. Les lignes `matchbox_socket` (en `DEBUG`) donnent le détail de la crate (« created offer », « Ice gathering completed »…).

**`chrome://webrtc-internals`** : ouvre-le dans un onglet **avant** de rejoindre la partie, puis rejoins-la dans un autre onglet. Chaque connexion a sa ligne (l'adresse du jeu) ; clique dessus et regarde :

- la configuration (`iceServers`) ;
- « ICE connection state » et « Connection state » (`new => checking => connected`, ou `failed`) ;
- la **grille des candidats** (« ICE candidate grid ») : **vide, c'est qu'aucun candidat n'a été trouvé** (pistes 1 et 2) ;
- les événements `setLocalDescription` / `setRemoteDescription` et `icecandidate`.

À envoyer si ça ne marche toujours pas : ces quatre éléments (une capture suffit), ou le fichier de **Create a WebRTC-Internals dump** (en haut de la page), avec la console du jeu et le terminal de `matchbox_server`.

### Sur la version en ligne (GitHub Pages)

La page est en `https`, et le serveur par défaut est `ws://localhost:3536` (non chiffré). Chrome l'accepte quand même pour `localhost` ; Firefox aussi en principe ; Safari le refuse sans doute (contenu mixte). Le plus sûr pour tester : `trunk serve` en local, comme plus haut.

### En natif

```bash
cargo run -- --room CODE                 # rejoint la partie CODE (L ouvre le salon)
cargo run -- --room CODE --pseudo Max --signal ws://192.168.1.20:3536
cargo run -- --room CODE --stun none     # sans STUN (le défaut avec une mise en relation locale)
```

Un test automatique fait se rencontrer deux clients natifs (serveur lancé ; `FLUFFY_STUN=auto` par défaut, `FLUFFY_STUN=stun:stun.l.google.com:19302` pour passer par un STUN) :

```bash
cargo test --lib net::scenario_tests::two_native_peers -- --ignored --nocapture
```

Deux pages Chromium qui se connectent pour de vrai (Node et Playwright, `matchbox_server` et `trunk serve` lancés) : `scripts/webrtc-pages.cjs` (voir son en-tête).

## Comment ce projet est construit

Le code est écrit par Claude Code, la plupart du temps en tâche de fond :

- [`CLAUDE.md`](CLAUDE.md) : règles, stack figée, conventions ;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) : kart, réseau P2P, personnages ;
- [`STATUS.md`](STATUS.md) : où on en est, décisions, questions ouvertes ;
- les **issues GitHub** forment le backlog : une issue = une PR.

## Crédits

Voir [`assets/CREDITS.md`](assets/CREDITS.md).
