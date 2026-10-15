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

On se retrouve dans le **salon** (touche **L**) : la liste des joueurs, le ping de chacun, l'hôte et le bouton « Prêt ». Une fois assis dans la partie, **tout le monde roule librement sur le circuit choisi par l'hôte** et **chacun voit les karts des autres**. Quand tout le monde est prêt, **l'hôte lance la course** (#9) : même circuit, même grille (les bots de l'hôte devant, les joueurs derrière), compte à rebours commun, classement partagé et mêmes résultats sur tous les écrans. **En ligne, rien ne met le jeu en pause.** Les **objets et les croquettes** sont partagés (#53) : l'hôte décide qui a pris une boîte ou une croquette, le lanceur d'un objet fait autorité sur sa trajectoire, et c'est la fenêtre du joueur touché qui décide s'il est touché.

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
7. **Rouler ensemble** (#8) : environ 2,5 s après l'arrivée dans la partie, chaque fenêtre passe sur **Prairie Pompon en roulage libre** (le circuit de l'hôte ; la console écrit `network: free driving on circuit 0, from grid slot N`). Ferme l'écran « Pilote et kart » (**Entrée**) dans les deux fenêtres. Chaque kart part de la place de la grille de son siège : le premier arrivé en pole, le second à côté. Ce que tu dois voir :
   - roule dans une fenêtre (flèches, ou Z/Q/S/D) : dans l'autre, ton kart avance **en douceur**, environ un dixième de seconde en retard, avec ton animal et la couleur que tu as choisie (point 10) ;
   - drift (Espace) : l'autre fenêtre montre la glisse et les étincelles bleues, oranges, violettes ; un boost (B, ou un mini-turbo) montre les flammes ; R te ramène sur ta place de la grille, et l'autre fenêtre te voit **sauter** jusqu'à elle, sans glisser ;
   - fonce dans l'autre kart : **ton** kart est repoussé, l'autre ne bouge pas sur ton écran (c'est son joueur qui le conduit) ; sur l'autre écran, c'est son kart qui est repoussé par le tien. Pas d'éjection, pas de kart qui monte sur l'autre ;
   - **Quitter la partie** : tu reviens au circuit et au mode d'avant ;
   - la console (F12) écrit toutes les 3 s où est ton kart (`network: our kart at (x, y, z)`) et où tu vois chacun des autres (`network: kart 1 of #abcd at …`, avec l'id du kart et le nombre d'états reçus) : de quoi comparer les deux fenêtres.
8. **La course en ligne** (#9) :
   - **le circuit** : dans le salon de l'hôte (« hôte » à côté de son nom), la liste **Circuit** : choisis « Anneau Guimauve » ; les deux fenêtres passent dessus en roulage libre (« Roulage libre sur Anneau Guimauve » dans le salon). Remets « Prairie Pompon ». Chez l'autre joueur, le salon dit « L'hôte choisit le circuit et lance la course quand tout le monde est prêt » ;
   - **le départ** : clique sur **Prêt !** dans les deux fenêtres, puis **Lancer la course** dans celle de l'hôte (le bouton n'est actif que quand tout le monde est prêt). Les deux fenêtres passent sur la grille en même temps : 10 bots devant, puis les deux joueurs côte à côte (11e et 12e places, dans leur ordre d'arrivée dans la partie) ; 2 s plus tard, la grille puis « 3, 2, 1, GO » **au même moment dans les deux fenêtres** (la console écrit `network: race 1 on circuit 0, 10 bots, from grid slot N, starting at tick T`, le même T des deux côtés). Le départ fusée marche comme en solo ;
   - **les bots** : ce sont ceux de l'hôte : dans l'autre fenêtre, les mêmes animaux aux mêmes places, qui roulent pareil (un dixième de seconde en retard) ;
   - **la course** : le classement en haut à gauche compte les 12 coureurs, les deux joueurs compris ; il peut différer d'une place entre les deux fenêtres quand deux karts sont au coude à coude (chacun voit l'autre un dixième de seconde dans le passé). Tu peux te coller dans le sillage de l'autre joueur (aspiration), lui donner un **coup de patte** (X ou F) ou **l'attraper** (tenir X) : c'est **sa** fenêtre qui décide si le coup porte, puis le « pof » s'affiche des deux côtés ;
   - **l'arrivée** : quand les deux joueurs ont fini (ou 30 s après le premier), l'hôte termine la course : « FINI ! », puis **le même classement dans les deux fenêtres**, avec les temps, ton nom (« Toi ») et celui de l'autre. La console de l'hôte écrit `network: the race is over at race tick …`. Pour la course suivante : **Lancer la course** dans le salon de l'hôte (ou un autre circuit) ;
   - **pas de pause** : ouvre le menu d'options (**O**) ou l'écran « Pilote et kart » (**G**) en pleine course : le jeu continue, ton kart roule en roue libre et l'autre fenêtre le voit avancer. Un nouveau kart choisi dans « Pilote et kart » sert à la course suivante ;
   - **l'hôte s'en va** : ferme la fenêtre de l'hôte en pleine course : l'autre devient l'hôte, **reprend les bots** (ils continuent de rouler, de là où ils étaient) et terminera la course ; le kart parti est classé dernier ;
   - un joueur qui arrive pendant une course roule librement à côté et la regarde ; il sera de la suivante.
9. **Les objets et les croquettes en ligne** (#53), en course (ou déjà en roulage libre) :
   - **les boîtes** : roule dans une boîte (la première ligne est sur la ligne droite de départ) : ta roulette tourne tout de suite (case en haut à gauche), et **la boîte disparaît aussi dans l'autre fenêtre**, puis revient 2 s plus tard au même moment dans les deux. Les deux karts dans la même boîte au même instant : un seul garde son objet (l'autre voit sa roulette s'arrêter, vide). La console des deux fenêtres écrit toutes les 3 s ce qui s'est passé (`network: items: 4 boxes and 6 croquettes taken, 3 items out, 1 hits, 1 croquette losses`) : les mêmes nombres des deux côtés, à un objet en route près ;
   - **lancer** (E ou Shift ; avec Bas/S ou C : derrière) : une **pelote** sur l'autre joueur part de ton kart et roule **au même endroit dans l'autre fenêtre** ; c'est **sa** fenêtre qui décide s'il est touché : s'il l'est, il fait une culbute dans les deux fenêtres, perd 3 croquettes (elles s'éparpillent au même endroit des deux côtés) puis clignote (invincible) ; touché pendant qu'il clignote, la pelote casse sans rien lui faire. Dans **ta** fenêtre, la pelote peut traverser son kart un dixième de seconde avant de disparaître : c'est le temps que sa fenêtre te réponde ;
   - **l'abeille** : derrière l'autre joueur, lance-la : elle file vers le kart juste devant toi au classement (le même dans les deux fenêtres) et lui fait un tête-à-queue ;
   - **les boucliers** : tiens E avec une pelure ou une pelote : elle reste derrière ton kart, **visible dans l'autre fenêtre** ; un objet lancé par l'autre sur ton bouclier casse avec lui, et ton kart n'est pas touché. Un « x3 » sorti tourne autour de ton kart, des deux côtés ;
   - **les croquettes** : ramasse celles de la piste : elles disparaissent aussi dans l'autre fenêtre (et reviennent 10 s plus tard, au même moment) ; celles qu'un kart perd (choc, coup de patte, objet) s'éparpillent au même endroit dans les deux fenêtres, et une croquette ramassée par l'un disparaît chez l'autre ;
   - **le piment et la croquette dorée** : l'autre fenêtre voit les flammes de ton boost ;
   - **les objets de phase 2 en ligne** (#18) : dans une fenêtre, **I** met un objet dans ta main (le suivant à chaque appui ; en ligne, seulement avec `trunk serve` ou `cargo run`, et seulement le piment doré, la bombe à confettis et le klaxon-canard, avec les objets de #16) :
     - **la bombe** (boule bleu marine, mèche jaune) : lancée devant l'autre joueur, elle part en cloche et explose là où elle retombe, au même moment dans les deux fenêtres ; s'il est à moins de 6 m, il fait une culbute (c'est **sa** fenêtre qui décide). Posée derrière toi (avec Bas/S), elle explose sur l'autre s'il roule dessus, au même endroit dans les deux fenêtres, ou au bout de 3 s ;
     - **le klaxon-canard** : les objets à moins de 9 m de ton kart disparaissent dans les deux fenêtres (une pelure posée par l'autre, ce qu'il traîne derrière lui), et l'autre fenêtre voit l'onde orange ;
     - **le piment doré** : 7 s de boosts, un par appui ; l'autre fenêtre voit tes flammes ;
     - la console écrit combien de bombes et de klaxons sont passés (`… items out (2 bombes), 1 klaxons …`), les mêmes nombres des deux côtés ;
   - **les bots de l'hôte** prennent des boîtes et lancent leurs objets comme en solo, dans les deux fenêtres ;
   - **l'hôte s'en va** : l'autre décide des boîtes et des croquettes à sa place ; les objets lancés par l'hôte parti disparaissent.
10. **Les couleurs** (#55) : dans chaque fenêtre, **G** (« Pilote et kart »), monte sur la dernière ligne, **Couleur** (Haut depuis la première ligne), choisis avec Gauche / Droite (ou clique sur une pastille), puis **Entrée** :
    - deux couleurs différentes : chaque fenêtre montre ton kart dans ta couleur et celui de l'autre dans la sienne ; la colonne **Couleur** du salon montre la pastille et le nom de chacun ;
    - **la même couleur** des deux côtés : le premier arrivé dans la partie la garde, l'autre prend la couleur libre la plus proche (rouge → orange, bleu → violet, rose → violet…), **la même dans les deux fenêtres** ; son salon écrit « Couleur déjà prise : tu es en orange au lieu de rouge. », et la colonne **Couleur** montre « (rouge pris) » à côté de sa pastille. Si le premier quitte la partie, l'autre retrouve sa couleur ;
    - **en course**, les bots gardent leurs couleurs, sauf celui qui porterait la couleur d'un joueur : il en prend une libre, la même dans les deux fenêtres ;
    - la couleur est gardée : recharge la page, elle est toujours là (et en solo, le bot qui avait ta couleur en change aussi).
11. Même chose avec **Chrome + Safari** : ouvre la même adresse dans Safari. Pour voir sa console : Safari > Réglages > Avancés > « Afficher les fonctionnalités pour les développeurs web », puis Développement > Afficher la console JavaScript.

Si la connexion ne se fait pas, le salon le dit (« connexion directe en cours… », puis « n'aboutit pas » avec les pistes) : voir **Dépannage** plus bas.

### 3. Jouer sur le réseau local

Deux appareils sur le même Wi-Fi (ton Mac et un autre ordinateur, par exemple), ou deux navigateurs de ton Mac via son adresse sur le réseau.

1. Trouve l'adresse IP de ton Mac : `ipconfig getifaddr en0` (Wi-Fi ; si rien ne s'affiche, essaie `en1`), par exemple `192.168.1.20`.
2. Lance `matchbox_server`, puis `trunk serve` : le jeu écoute sur toutes les interfaces (`Trunk.toml`), et trunk affiche ses adresses au démarrage.
3. Sur les deux appareils, ouvre `http://192.168.1.20:8080/?room=TEST` (avec ton IP). La page trouve toute seule le serveur de mise en relation sur la même machine (`ws://192.168.1.20:3536`, affiché dans le salon), sans `&signal=`. Le lien d'invitation du salon reprend cette adresse.
4. Si macOS demande d'**accepter les connexions entrantes** (pour `matchbox_server` ou `trunk`) ou si le navigateur veut **accéder aux appareils du réseau local**, accepte.
5. Ce que tu dois voir : comme plus haut, les deux joueurs avec leur ping ; dans la console, `offer sent` / `answer received` puis `connected`. En bas du salon : « Mise en relation : ws://192.168.1.20:3536 » et « Connexion directe : sans serveur STUN ». Puis, comme au point 7 plus haut, Prairie Pompon sur les deux appareils : roule sur l'un, ton kart roule sur l'autre ; et la course du point 8 : **Prêt !** sur les deux appareils, **Lancer la course** chez l'hôte, avec les objets et les croquettes du point 9.

Ouverte en `localhost`, la page ne connaît pas l'adresse de ton Mac sur le réseau (le salon le rappelle) : pour inviter quelqu'un, ouvre-la avec l'adresse IP.

### 4. Par Internet

Il faut un serveur de mise en relation public en `wss://` (issue #10, en attente) ; `?signal=wss://…` le désigne. Le lien d'invitation garde ce paramètre.

Par Internet, chaque navigateur demande son adresse publique aux serveurs STUN publics de Google. Sans relais TURN, certains réseaux (entreprise, 4G) bloquent la connexion directe ; un relais se donne avec `?turn=turn:relais.exemple.org:3478&turnuser=NOM&turnpass=MOT_DE_PASSE` (celui de production viendra avec #10).

### Simuler un mauvais réseau (debug)

Pour voir comment les karts des autres se comportent sur une mauvaise connexion, le jeu peut ajouter de la latence, de la gigue et des pertes aux états des karts qu'il **reçoit** (pas au salon ni aux pings : le ping affiché reste le vrai) :

- dans l'adresse : `http://localhost:8080/?room=TEST&netsim=120,20,5` : 120 ms de latence, ± 20 ms de gigue, 5 % de pertes (`&netsim=80` : la latence seule) ;
- ou pendant le jeu, dans le panneau d'admin (**P**) : `NetTuning` > `simulator` (`latency_ms`, `jitter_ms`, `loss_percent`), effet immédiat. Remets-les à 0 (ou le bouton de remise à zéro de `NetTuning`) pour l'arrêter : le panneau garde les réglages modifiés d'une session à l'autre. Attention : avec `?netsim=` dans l'adresse, si tu modifies un autre réglage dans le panneau, le simulateur est mémorisé avec ;
- chaque fenêtre simule son côté : pour ralentir les deux sens, mets `netsim` dans les deux adresses.

Ce que tu dois voir avec `netsim=120,20,5` : le kart de l'autre roule toujours **sans à-coups**, simplement un peu plus en retard (environ 0,2 s au lieu de 0,1 s). Les réglages de l'affichage (délai de 100 ms, extrapolation de 250 ms au plus…) sont aussi dans `NetTuning`.

### Réglages de la connexion

| Page | Natif | Effet |
|---|---|---|
| `?signal=ws://…` | `--signal ws://…` | Le serveur de mise en relation. Par défaut : celui de la machine qui sert la page quand elle est locale, sinon `ws://localhost:3536`. |
| `?stun=none` | `--stun none` | Aucun serveur STUN (le défaut quand la mise en relation est locale). |
| `?stun=stun:hôte:port,…` | `--stun …` | Ces serveurs STUN. `?stun=auto` : le défaut. |
| `?turn=turn:hôte:port&turnuser=…&turnpass=…` | `--turn … --turnuser … --turnpass …` | Un relais TURN (les trois sont nécessaires). Aucun par défaut. |
| `?netsim=120,20,5` | `--netsim 120,20,5` | Simulateur de réseau (debug) : latence (ms), gigue (ms), pertes (%) des états de karts reçus. |

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

Un autre fait rouler deux jeux complets (sans fenêtre) dans la même partie : Ana roule, et le test compare, tick par tick, où Bob voit son kart et où il était (`FLUFFY_NETSIM=120,20,5` met le simulateur chez Bob) :

```bash
cargo test --lib net::scenario_tests::two_games_drive -- --ignored --nocapture
```

Un troisième fait courir deux jeux complets l'un contre l'autre (#9) : l'hôte lance une course de 3 tours avec 10 bots, les deux joueurs sont conduits par des bots jusqu'à l'arrivée, et le test vérifie que les deux jeux s'accordent (circuit, tick de départ, bots, GO, tours, arrivées, classement final) :

```bash
cargo test --lib net::race_tests::two_games_race -- --ignored --nocapture
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
