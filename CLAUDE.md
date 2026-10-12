# Fluffy Racers : instructions pour Claude

Jeu de kart arcade multijoueur dans le navigateur, avec des animaux tout mous et rigolos. Rust + Bevy, compilé en WebAssembly.

## Mode de travail (lis ceci en premier)

Tu travailles **sans surveillance**, en général lancé par une routine planifiée. Max ne relit pas chaque changement.

### Branches
- **`dev`** est la branche par défaut. Ton travail y arrive automatiquement, et c'est elle qui est déployée sur GitHub Pages pour que Max teste.
- **`main`** est la version stable. Seul Max y fusionne `dev`, quand il a joué.
- **Toi, tu pousses uniquement des branches `claude/issue-<n>-<slug>`.** Ne pousse jamais sur `dev` ni sur `main`.
- Quand la CI passe sur ta branche, `.github/workflows/automerge.yml` ouvre la PR vers `dev` (si elle n'existe pas), la fusionne en squash, supprime la branche et redéploie. Si la CI échoue, il ouvre une issue avec le label `ci-failure`.
- **La CI est le seul garde-fou avant `dev`.** Tes tests doivent donc vraiment vérifier le comportement.

### Déroulé d'une session
1. **État des lieux.** Lance `git fetch origin --prune`, lis `STATUS.md` et les issues ouvertes avec le label `ci-failure`.
   - S'il reste une branche distante `claude/*`, c'est une fusion en attente ou ratée.
   - Si son dernier commit a moins de 30 min, attends sa fusion (étape 6).
   - Sinon, reprends-la : rebase sur `origin/dev`, corrige, revérifie, repousse.
2. **Choisir l'issue** (si le prompt n'en impose pas une) : l'issue ouverte au plus petit numéro dans le plus petit milestone (`milestone-1`, puis 2, puis 3), sans label `needs-max` ni `ci-failure`.
3. **Créer la branche** `claude/issue-<n>-<slug>` depuis `origin/dev`.
4. **Vérifier en local.** Avant chaque push, tout doit passer. Ne pousse jamais une branche qui échoue chez toi.
   ```
   cargo fmt
   cargo clippy --all-targets -- -D warnings
   cargo test
   cargo check --target wasm32-unknown-unknown
   ```
5. **Écrire le dernier commit de la branche comme une description de PR**, puisque c'est lui qui sert à créer la PR.
   - Titre court.
   - Corps avec :
     - **Comment tester** : étapes concrètes pour Max (lien du jeu, touches, ce qu'il doit observer) ;
     - **Vérifié / non vérifié** : sois honnête sur ce que tu n'as pas pu voir, puisque tu ne peux pas jouer ;
     - les réglages ajoutés et leurs valeurs ;
     - `Closes #<n>`.
6. **Pousser, puis attendre la fusion.**
   - Lance `git fetch origin` toutes les 2 min jusqu'à ce que ton travail soit dans `origin/dev` (45 min maximum).
   - Pour voir l'état de la CI : `gh run list --branch <branche>` si `gh` fonctionne, sinon `curl -s "https://api.github.com/repos/MaximeChaillou/fluffy-racers/actions/runs?branch=<branche>&per_page=1"`.
   - Si la CI échoue, corrige sur la même branche et repousse.
7. **Enchaîner.** Une fois la branche fusionnée, repars de `origin/dev` pour l'issue suivante, dans la limite fixée par le prompt (3 issues par session par défaut).
8. **En cas de blocage** (3 échecs de CI sur la même issue, décision qui revient à Max, action de Max requise) :
   - explique-le dans le message de commit et dans `STATUS.md` (section « Questions pour Max ») ;
   - laisse la branche en place ;
   - passe à une issue indépendante si possible, sinon arrête-toi.
9. **Terminer la session** par un résumé : ce qui a été fusionné, ce qui a échoué, et les questions pour Max.

### Règles permanentes
- **Une issue = une branche = une PR.** Fais de petits incréments qui fonctionnent. Si une issue est trop grosse, livre une première étape utile. Pour la suite, crée une issue si tu peux, sinon note-la dans `STATUS.md`.
- **Ne t'arrête pas pour poser une question.** Pour un choix mineur, décide et note la décision dans `STATUS.md`.
- **Chaque branche met à jour `STATUS.md`** : ce qui marche, les décisions prises, la suite, et les questions éventuelles.
- **Commit `Cargo.lock`** (c'est une application, pas une bibliothèque).
- **Ne casse pas l'existant.** Tous les tests déjà en place doivent continuer à passer.

## Vision du jeu

- **Le ressenti de référence est Mario Kart 8 Deluxe** : drift et mini-turbos à 3 paliers, départ fusée, figures, aspiration, croquettes (l'équivalent des pièces), objets dosés selon la distance au leader, 12 coureurs, 3 tours.
- **`docs/GAME_DESIGN.md` est la source de vérité du gameplay.** Lis la section concernée avant toute issue de gameplay. En cas de conflit avec le texte d'une issue, c'est le game design qui l'emporte : signale l'écart dans la PR.
- **4 joueurs minimum en ligne, 12 visés.** Des bots complètent la grille.
- Personnages : animaux ronds, mous et maladroits. L'humour physique compte (rebonds, wobble, ragdoll léger quand on se fait toucher).
- **On reprend les mécaniques, jamais le contenu.** Aucun nom, personnage, objet, logo, tracé de circuit, musique ou son de Mario Kart, Party Animals ni d'aucune autre marque. Utilise les équivalents originaux du game design : pelure, pelote, abeille, piment, croquettes, pélican dépanneur…
- **La fluidité est la priorité n°1** : 60 fps stables dans Chrome, Firefox et Safari sur un portable moyen, avec 12 karts.
- **Les valeurs « (à régler) »** du game design vont dans des Resources de réglage avec ces valeurs par défaut. Ne les code jamais en dur.

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

- Ne pousse jamais sur `dev` ni sur `main`, et ne force-push jamais.
- **Ne modifie pas `.github/workflows/`** (CI, fusion automatique, déploiement) ni la configuration de lint, sauf si une issue créée par Max le demande explicitement.
- Ne supprime ni ne désactive de tests, et n'affaiblis pas les vérifications, pour faire passer la CI.
- Ne change pas la version de Bevy ni la stack ci-dessus.
- Pas d'asset sous licence non libre, pas de contenu de marques existantes.
