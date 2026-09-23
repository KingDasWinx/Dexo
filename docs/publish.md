# Como publicar uma versão nova do Dexo

Guia de consulta para lançar versões. Resumo: **faça o merge no `main`, rode `./publish.sh` e espere uns 15 minutos.** O resto é automático.

## Como funciona

```
./publish.sh minor  →  commit + tag v1.3.0  →  GitHub Actions (release.yml)
                                                   │
         ┌─────────────────────────────────────────┼──────────────────────────────┐
         ▼                    ▼                    ▼                              ▼
  GitHub Release         Homebrew tap         Scoop bucket            winget PR + .deb/.rpm
  (binários, SBOM,      (dexo.rb)            (dexo.json)             (depois da Release)
   instaladores)
```

1. O `publish.sh` sobe a versão no `Cargo.toml`, escreve a seção nova do `CHANGELOG.md`, cria o commit `chore(release): vX.Y.Z` e a tag `vX.Y.Z`, e dá push.
2. A tag dispara o `.github/workflows/release.yml`, que compila os 5 sistemas (Linux x86_64/ARM64, macOS Intel/Apple Silicon e Windows x86_64) e cria a Release no GitHub.
3. Com a Release criada, os gerenciadores de pacote são atualizados sozinhos.

## Passo a passo

### 1. Deixe o `main` pronto

- Faça o merge do PR `development` → `main` com o CI verde.
- Tudo o que está no `main` entra na versão.

### 2. Escolha o tipo de versão

| Comando | Quando usar | Exemplo |
|---|---|---|
| `./publish.sh patch` | Só correções de bugs | 1.2.0 → 1.2.1 |
| `./publish.sh minor` | Funcionalidades novas, sem quebrar nada | 1.2.0 → 1.3.0 |
| `./publish.sh major` | Algo que quebra o jeito antigo de usar | 1.2.0 → 2.0.0 |
| `./publish.sh 1.3.0` | Quando você quer uma versão exata | → 1.3.0 |

### 3. Rode

```sh
git checkout main
git pull
./publish.sh minor
```

O script mostra a versão nova e o changelog gerado, e pergunta:

```
Publish v1.3.0? [y/N]
```

- `y` publica.
- Qualquer outra resposta cancela, e **nada é alterado**.

Para ver o que aconteceria sem mudar nada:

```sh
./publish.sh minor --dry-run
```

### 4. Acompanhe

O script termina mostrando o link do Actions. Leva uns 15 minutos, principalmente por causa do macOS e do Windows. Os jobs, em ordem:

| Job | O que faz |
|---|---|
| `plan` | Confere se a tag bate com a versão e monta o plano |
| `build-local-artifacts` (5×) | Compila cada sistema |
| `build-global-artifacts` | Gera os instaladores, a fórmula do Homebrew e o SBOM |
| `host` | **Cria a GitHub Release** com todos os arquivos |
| `publish-homebrew-formula` | Atualiza o `KingDasWinx/homebrew-tap` |
| `custom-publish-scoop` | Atualiza o `KingDasWinx/scoop-bucket` |
| `announce` | Finaliza a publicação |
| `custom-package-linux` | Anexa os `.deb`/`.rpm` na Release |
| `custom-publish-winget` | Abre o PR no `microsoft/winget-pkgs` (se o winget estiver ligado) |

### 5. Confira

- [ ] A [Release](https://github.com/KingDasWinx/Dexo/releases) nova aparece como **Latest**.
- [ ] Ela tem os `.tar.gz`/`.zip`, os `.deb`/`.rpm`, os instaladores e o `dexo.cdx.xml`.
- [ ] O [homebrew-tap](https://github.com/KingDasWinx/homebrew-tap) e o [scoop-bucket](https://github.com/KingDasWinx/scoop-bucket) têm um commit com a versão nova.
- [ ] Se o winget estiver ligado, existe um PR novo em [winget-pkgs](https://github.com/microsoft/winget-pkgs/pulls?q=KingDasWinx.Dexo). O merge é feito pela Microsoft, e costuma ser automático depois da validação.

## Como o changelog é gerado

O changelog sai **das mensagens de commit** desde a última versão estável:

| Commit começa com | Vai para |
|---|---|
| `feat:` ou `feat(escopo):` | **Features** |
| `fix:` | **Fixes** |
| `perf:` | **Performance** |
| sem prefixo nenhum | **Other changes** |
| `docs:`, `test:`, `chore:`, `style:`, `ci:`, `build:`, `refactor:` | Fica de fora |

Exemplo: `fix(tui): the tab strip lights the open document` vira `- The tab strip lights the open document` em **Fixes**.

**Dica:** escreva a mensagem do commit pensando em quem usa o Dexo. Ela vira a nota da versão, e um commit chamado "Tudo" aparece no changelog como "Tudo".

## Ensaio (pré-release)

Para testar o pipeline sem publicar nos gerenciadores de pacote:

```sh
./publish.sh 1.3.0-rc.1
```

- **Faz:** compila tudo e cria uma Release marcada como *pre-release*.
- **Não faz:** não publica no Homebrew, no Scoop nem no winget, não mexe no `CHANGELOG.md` e não gera os `.deb`/`.rpm` (limitação do GitHub Actions: jobs que dependem de um job pulado também são pulados).
- Depois, `./publish.sh minor` a partir de `1.3.0-rc.1` lança a `1.3.0`, não a `1.4.0`. O changelog da `1.3.0` inclui tudo desde a última versão estável.

## Quando algo dá errado

### O `publish.sh` recusou rodar

| Mensagem | O que fazer |
|---|---|
| `releases are cut from main` | `git checkout main` |
| `the working tree has uncommitted changes` | Faça commit ou `git stash` das alterações. Arquivos não rastreados não atrapalham |
| `main is not in sync with origin/main` | `git pull`, ou `git push` se você tem commits locais |
| `X is not newer than Y` | A versão pedida é menor ou igual à atual; escolha outra |
| `tag vX already exists` | Essa versão já foi lançada; use a próxima |

Em todos esses casos, nada foi alterado.

### Um job falhou no Actions

- **Não crie outra versão.** Abra a execução no Actions e clique em **Re-run failed jobs**.
- Se falhar o **Homebrew** ou o **Scoop**, a GitHub Release já foi publicada, mas os `.deb`/`.rpm` e o winget **não rodam**, porque dependem de todos os anteriores. Depois do re-run, eles rodam.
- Se falhar um **build**, nada foi publicado. Corrija o código no `main` e lance a próxima versão, porque a tag com problema fica sem Release.

### Erro 401/403 no Homebrew, no Scoop ou no winget: o token expirou

Os tokens têm validade. Quando vencem, a publicação nos gerenciadores falha.

- **`HOMEBREW_TAP_TOKEN`** (serve para o Homebrew e o Scoop): crie outro em [tokens fine-grained](https://github.com/settings/personal-access-tokens/new), com acesso só a `homebrew-tap` e `scoop-bucket` e **Contents: Read and write**.
- **`WINGET_TOKEN`**: crie outro em [tokens clássicos](https://github.com/settings/tokens/new?scopes=public_repo,workflow&description=dexo-winget), com `public_repo` e `workflow`.
- Salve o valor novo por cima do antigo em [Settings → Secrets → Actions](https://github.com/KingDasWinx/Dexo/settings/secrets/actions) e depois faça o **Re-run failed jobs**.

### Lancei uma versão com bug

**Não apague a versão.** Os gerenciadores de pacote já apontam para ela, e apagar a Release quebra a instalação de quem ainda não atualizou. Corrija e lance um `./publish.sh patch`.

## winget: ligar e desligar

O job do winget só roda quando a variável `WINGET_ENABLED` vale `true`:

- **Ligar:** [Settings → Secrets and variables → Actions → Variables](https://github.com/KingDasWinx/Dexo/settings/variables/actions) → *New repository variable* → nome `WINGET_ENABLED`, valor `true`.
- Ligue **só depois** que o primeiro PR ([#440056](https://github.com/microsoft/winget-pkgs/pull/440056)) for mergeado pela Microsoft. A automação só consegue atualizar um pacote que já existe lá.
- **Desligar:** apague a variável ou mude o valor para `false`.

## Onde fica cada coisa

| Arquivo | O que é |
|---|---|
| `publish.sh` | O script de lançamento |
| `dist-workspace.toml` | A configuração do release: targets, instaladores, Homebrew e jobs extras |
| `.github/workflows/release.yml` | **Gerado pelo dist. Nunca edite à mão** (veja abaixo) |
| `.github/workflows/publish-scoop.yml` | Atualiza o Scoop |
| `.github/workflows/publish-winget.yml` | Abre o PR no winget |
| `.github/workflows/package-linux.yml` | Gera e anexa os `.deb`/`.rpm` |
| `packaging/nfpm.yaml` | Metadados dos pacotes `.deb`/`.rpm` (nome, dependências, licenças) |

### Mudar algo no pipeline

O `release.yml` é gerado a partir do `dist-workspace.toml` pela ferramenta [dist](https://github.com/axodotdev/cargo-dist). Para mudar o release:

1. Edite o `dist-workspace.toml`, ou um dos jobs `publish-*`/`package-linux`, que são seus e podem ser editados.
2. Regere o workflow com a mesma versão do dist que está em `cargo-dist-version`:
   ```sh
   dist generate
   ```
   Se não tiver o `dist` instalado (a 0.33.0 não está no crates.io, só nas releases do GitHub):
   ```sh
   curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.33.0/cargo-dist-installer.sh | sh
   ```
3. `dist generate --check` confere se o `release.yml` está em dia. O PR roda o job `plan`, que pega config quebrada antes do merge.

## Como o usuário instala

| Sistema | Comando |
|---|---|
| macOS / Linux (Homebrew) | `brew install kingdaswinx/tap/dexo` |
| Windows (Scoop) | `scoop bucket add dexo https://github.com/KingDasWinx/scoop-bucket` e depois `scoop install dexo` |
| Windows (winget, depois do merge) | `winget install KingDasWinx.Dexo` |
| Debian / Ubuntu | Baixar o `.deb` da Release e rodar `sudo apt install ./dexo_*_amd64.deb` |
| Fedora | Baixar o `.rpm` da Release e rodar `sudo dnf install ./dexo-*.x86_64.rpm` |
| Qualquer Unix | `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/kingdaswinx/Dexo/releases/latest/download/dexo-installer.sh \| sh` |

## Limitações conhecidas

- **glibc 2.35 ou mais nova no Linux:** os binários são compilados no Ubuntu 22.04. Funcionam no Ubuntu 22.04+, Debian 12+ e Fedora 36+, mas **não no RHEL/Rocky 9** (glibc 2.34).
- **Binários não assinados:** no macOS, um binário baixado **pelo navegador** pode ser bloqueado pelo Gatekeeper. No Windows, o SmartScreen pode avisar. Pelo Homebrew, pelo Scoop, pelo winget e pelos instaladores via `curl`/`irm` isso não acontece.
- **Os `.deb`/`.rpm` não se atualizam sozinhos:** não existe repositório apt/dnf. O usuário baixa a versão nova e instala do mesmo jeito.
- **Sem crates.io:** o Dexo depende do `ttfx` direto do Git, e o crates.io não aceita isso.
