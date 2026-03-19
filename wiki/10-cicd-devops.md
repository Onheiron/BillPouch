# 10. CI/CD & DevOps

## Panoramica workflow GitHub Actions

| Workflow            | Trigger                             | Scopo                                   |
|---------------------|-------------------------------------|-----------------------------------------|
| **CI**              | push/PR → `main`                    | fmt, clippy, test (Linux + macOS)       |
| **Smoke**           | push/PR → `main`                    | Test P2P con 3 nodi Docker              |
| **Coverage**        | push/PR → `main`                    | cargo-tarpaulin → Codecov               |
| **Docs**            | push → `main`                       | cargo doc → GitHub Pages                |
| **Security**        | push → `main` + ogni lunedì 08:00   | cargo-audit + cargo-deny                |
| **Release**         | push tag `v*` o `billpouch-v*`      | Build binari cross-platform             |
| **Release Please**  | push → `main`                       | Gestione PR release + build artefatti   |
| **CommitLint**      | PR → `main`                         | Verifica conventional commits           |

---

## CI

Matrix su **ubuntu-latest** e **macos-latest**:

```yaml
steps:
  - checkout
  - install Rust stable (rustfmt + clippy)
  - cache cargo registry e target/
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo test --workspace
```

Il warning `clippy` è trattato come **errore bloccante** (`-D warnings`).

---

## Smoke Test

Avvia il cluster Docker con 3 nodi e verifica la discovery P2P:

```yaml
steps:
  - docker compose -f docker-compose.smoke.yml up --build -d
  - ./smoke/smoke-test.sh
  - docker compose -f docker-compose.smoke.yml down
```

---

## Coverage

```yaml
steps:
  - cargo install cargo-tarpaulin
  - cargo tarpaulin --packages bp-core --test architecture_test --out xml
  - codecov/codecov-action (token: CODECOV_TOKEN)
```

Output XML in formato Cobertura. `fail_ci_if_error: false` per Codecov
(advisory, non bloccante).

---

## Documentazione (GitHub Pages)

```yaml
steps:
  - RUSTDOCFLAGS="--deny warnings" cargo doc --no-deps --workspace
  - aggiunta redirect index.html → bp_core/index.html
  - upload artifact GitHub Pages
  - deploy con concurrency "pages" (annulla runs precedenti)
```

URL risultante: **https://onheiron.github.io/BillPouch/**

---

## Security Audit

Entrambi i job hanno `continue-on-error: true` (advisory, non bloccano la CI):

```yaml
cargo-audit:
  - cargo install cargo-audit
  - cargo audit

cargo-deny:
  - cargo install cargo-deny
  - cargo deny check
```

Schedule: ogni **lunedì alle 08:00 UTC**.

---

## Release

### Flusso automatico con release-please

1. Ogni commit su `main` viene analizzato da **release-please**
2. Viene creata/aggiornata una PR "Release vX.Y.Z"
3. Alla merge della PR, release-please:
   - Aggiorna `version.txt` e `.release-please-manifest.json`
   - Taglia il tag `billpouch-vX.Y.Z`
   - Crea la Release su GitHub
4. Il tag trigghera il build dei binari cross-platform

### Target di build

| Target                     | Runner         | Artefatto            |
|----------------------------|----------------|----------------------|
| `x86_64-unknown-linux-gnu` | ubuntu-latest  | `bp-linux-x86_64`    |
| `x86_64-apple-darwin`      | macos-latest   | `bp-macos-x86_64`    |
| `aarch64-apple-darwin`     | macos-latest   | `bp-macos-aarch64`   |

Tutti i binari vengono uplodati alla Release GitHub.

---

## Conventional Commits

Il progetto usa **Conventional Commits** enforciato via CommitLint nelle PR.
Il CHANGELOG è generato automaticamente da **git-cliff** (`cliff.toml`):

| Prefisso    | Sezione CHANGELOG    |
|-------------|----------------------|
| `feat`      | Features             |
| `fix`       | Bug Fixes            |
| `doc`       | Documentation        |
| `perf`      | Performance          |
| `refactor`  | Refactoring          |
| `style`     | Styling              |
| `test`      | Testing              |
| `ci`        | CI/CD                |
| `build`     | Build                |
| `chore`     | Miscellaneous        |

---

## Versioning

| File                              | Contenuto                              |
|-----------------------------------|----------------------------------------|
| `version.txt`                     | Versione corrente (`0.1.3`)            |
| `.release-please-manifest.json`   | Manifest release-please (`"." : "0.1.3"`) |
| `release-please-config.json`      | Configurazione release-please          |
| `cliff.toml`                      | Configurazione git-cliff               |
| `Cargo.toml` workspace            | `version = "0.1.0"` (allineato manualmente) |
