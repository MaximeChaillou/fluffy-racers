# Fluffy Racers : instructions pour Claude

Jeu de kart arcade multijoueur dans le navigateur, avec des animaux tout mous et rigolos. Rust + Bevy, compilé en WebAssembly.

## Mode de travail (lis ceci en premier)

Tu travailles le plus souvent **sans surveillance** : session cloud ou tâche planifiée. Max lance une tâche et revient plus tard.

- **Une issue GitHub = une branche = une PR.** Nomme la branche `issue-<n>-<slug>`. La PR contient `Closes #<n>`. Ne pousse jamais directement sur `main`.
- **Ne t'arrête pas pour poser une question** sauf blocage réel. Dans ce cas, écris la question dans `STATUS.md` (section « Questions pour Max ») et dans la PR, puis avance sur ce qui n'est pas bloqué. Pour les choix mineurs, décide toi-même et note la décision.
- **Choisir la prochaine issue** (si on ne t'en a pas donné une) : l'issue ouverte au plus petit numéro dans le plus petit milestone (`milestone-1`, puis 2, puis 3), sans label `needs-max` et sans PR déjà ouverte.
- **Commit `Cargo.lock`** (c'est une application, pas une bibliothèque).
- **Petites PR, toujours compilables.** Si une issue est trop grosse, livre une première étape qui fonctionne, puis crée une issue de suite.
- **Avant chaque push**, tout doit passer :
  ```
  cargo fmt
  cargo clippy --all-targets -- -D warnings
  cargo test
  cargo check --target wasm32-unknown-unknown
  ```
- **Chaque PR met à jour `STATUS.md`** : ce qui marche, les décisions prises, la suite, et les questions éventuelles.
- **Chaque description de PR contient :**
  - **Comment tester** : étapes concrètes pour Max (commande ou lien, touches, ce qu'il doit observer).
  - **Vérifié / non vérifié** : sois honnête sur ce que tu n'as pas pu voir (tu ne peux pas jouer).
  - Les réglages de gameplay ajoutés et leurs valeurs, pour que Max puisse les ajuster.

## Vision du jeu

- Kart arcade, sessions courtes, fun immédiat. Jouable au clavier et à la manette.
- **4 joueurs minimum en ligne, 12 visés.** Des bots complètent la grille.
- Personnages : animaux ronds, mous et maladroits. L'humour physique compte (rebonds, wobble, ragdoll léger quand on se fait toucher).
- **Univers 100 % original.** N'utilise jamais les noms, personnages, logos, objets ou circuits de Mario Kart, Party Animals ni d'aucune autre marque.
- **La fluidité est la priorité n°1** : 60 fps stables dans Chrome, Firefox et Safari sur un portable moyen, avec 12 karts.

## Stack (figée : ne la change pas sans l'accord de Max)

| Domaine | Choix | Notes |
|---|---|---|
| Langage | Rust stable ≥ 1.97.1, édition 2024 | `rust-toolchain.toml` |
| Moteur | **Bevy 0.20** | Aucune autre version. Vérifie l'API 0.20 (docs.rs/bevy/0.20.0) plutôt que de te fier à ta mémoire : beaucoup d'API ont changé entre les versions. |
| Physique | **bevy_rapier3d 0.37** | Seule lib physique compatible Bevy 0.20 au 12/10/2026. |
| Réseau | **matchbox_socket 0.14** (WebRTC) + **notre propre netcode** | Indépendant de Bevy. Voir `docs/ARCHITECTURE.md`. |
| Build web | **trunk** | `index.html` à la racine. |
| Rendu web | WebGL2 (défaut de Bevy) | Compatibilité maximale. WebGPU sera évalué plus tard. |
| Serveurs | **Aucun serveur de jeu** | Seulement une signalisation WebRTC (voir l'architecture). |

**À ne pas ajouter** tant qu'ils ne supportent pas Bevy 0.20 : `avian3d`, `lightyear`, `bevy_ggrs`, `bevy_matchbox`. Si l'un d'eux passe en 0.20 et te semble utile, propose-le dans `STATUS.md` sans l'ajouter.

**Toute nouvelle crate** doit être justifiée dans la PR et compatible Bevy 0.20 + `wasm32-unknown-unknown`. Moins de dépendances = compilation plus rapide et wasm plus léger.

## Commandes

```bash
cargo run                                   # version native (fenêtre)
trunk serve                                 # version web sur http://localhost:8080
trunk build --release                       # build web de prod dans dist/
cargo test                                  # tests
cargo check --target wasm32-unknown-unknown # vérifie que ça compile pour le web
```

Multijoueur en local : `cargo install matchbox_server && matchbox_server` (port 3536), puis ouvre plusieurs onglets avec la même room.

Environnement cloud : `scripts/cloud-setup.sh` installe la toolchain, les libs système et trunk.

## Organisation du code

- `src/main.rs` : la fenêtre et l'ajout de `GamePlugin`, rien d'autre.
- `src/lib.rs` : `GamePlugin`, qui assemble les plugins de chaque domaine.
- **Un module (plugin) par domaine** : `kart/`, `track/`, `camera/`, `race/`, `net/`, `bots/`, `ui/`, `audio/`, `characters/`.
- **La logique pure est séparée des systèmes Bevy** (règles de course, sérialisation réseau, maths du kart) pour être testée unitairement.
- **La simulation tourne en `FixedUpdate` à 60 Hz.** Le rendu interpole entre deux ticks pour rester fluide à n'importe quel framerate.
- **Le kart ne lit jamais le clavier.** Des systèmes d'entrée produisent un `KartInput` par tick (accélérateur, frein, direction, drift, objet). Le kart ne consomme que `KartInput`. C'est ce qui permet le réseau, les bots et les replays.
- **Les réglages de gameplay** (vitesse, grip, drift, boost, gravité…) sont regroupés dans une `Resource` avec des valeurs par défaut documentées, faciles à ajuster.

## Performance

- Low-poly, une seule lumière directionnelle avec ombres, peu de matériaux.
- Pas d'allocation par frame dans les systèmes chauds. Réutilise les buffers.
- Surveille la taille du `.wasm` (affichée par la CI) et note-la dans `STATUS.md` quand elle change nettement.

## Assets

- Au départ, **packs CC0 uniquement** (Kenney, Quaternius…), au format glTF/GLB. Chaque asset ajouté a une ligne dans `assets/CREDITS.md`.
- La session cloud n'a pas forcément accès à kenney.nl ou quaternius.com. Si un téléchargement échoue, ne contourne pas : utilise des formes primitives et signale-le dans `STATUS.md`.

## Vérifier ton travail

Tu ne peux pas jouer, donc :
- tests unitaires sur la logique pure ;
- tests d'intégration headless de l'`App` (`MinimalPlugins`) : faire tourner N ticks et vérifier des positions, vitesses ou états ;
- pour le ressenti de conduite, des tests de scénario. Exemple : « accélérateur à fond 2 s sur le plat → vitesse entre X et Y », « drift 1,5 s → mini-turbo niveau 2 » ;
- si un navigateur headless est disponible, tu peux lancer `trunk serve` et faire une capture d'écran. Sinon, écris-le dans la PR.

## Interdits

- Pas de push sur `main`, pas de force-push sur une branche qui a une PR ouverte.
- Ne change pas la version de Bevy ni la stack ci-dessus.
- Ne supprime ni ne désactive de tests pour faire passer la CI.
- Pas d'asset sous licence non libre, pas de contenu de marques existantes.
