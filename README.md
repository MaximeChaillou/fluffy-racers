# Fluffy Racers

Jeu de kart multijoueur dans le navigateur, avec des animaux tout mous. Rust + Bevy, compilé en WebAssembly.

**Jouer :** https://maximechaillou.github.io/fluffy-racers/ (mis à jour à chaque merge sur `main`)

## Développer

Prérequis : Rust stable (installé via [rustup](https://rustup.rs)) et [trunk](https://trunkrs.dev).

```bash
cargo run      # version native
trunk serve    # version web sur http://localhost:8080
cargo test
```

Sur Linux, il faut aussi : `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

## Comment ce projet est construit

Le code est écrit par Claude Code, la plupart du temps en tâche de fond :

- [`CLAUDE.md`](CLAUDE.md) : règles, stack figée, conventions ;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) : kart, réseau P2P, personnages ;
- [`STATUS.md`](STATUS.md) : où on en est, décisions, questions ouvertes ;
- les **issues GitHub** forment le backlog : une issue = une PR.

## Crédits

Voir [`assets/CREDITS.md`](assets/CREDITS.md).
