# Dexo Implementation Roadmap

> **For agentic workers:** Este índice coordena os planos de sprint. Cada plano exige `superpowers:subagent-driven-development` (recomendado) ou `superpowers:executing-plans`. Execute as sprints em ordem e marque os checkboxes no arquivo da sprint.

**Spec:** `docs/superpowers/specs/2026-08-14-dexo-product-design.md`

**Objetivo final:** entregar Dexo 1.0 completo, local-first, multiplataforma, com PostgreSQL, MySQL, TUI, CLI e servidor MCP governado.

## Regras de execução

1. Uma sprint começa somente com a anterior verde em `cargo nextest run --workspace --all-features` e `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
2. Cada task usa TDD: teste falhando, implementação mínima, teste verde e commit próprio.
3. Não avance deixando teste ignorado, placeholder, warning ou segredo em fixture.
4. Mudança de contrato público exige atualizar contract tests e todos os adapters na mesma sprint.
5. PostgreSQL/MySQL integration tests exigem Docker; o restante deve continuar executável sem Docker.
6. Ao terminar a sprint, execute o checklist de saída do plano e registre o commit final no topo do arquivo.

## Ordem das sprints

| Sprint | Plano | Resultado utilizável |
| --- | --- | --- |
| 00 | [Fundação e qualidade](2026-08-14-sprint-00-foundation-quality.md) | Workspace compila, CLI inicia e CI aplica gates |
| 01 | [Estado local e segredos](2026-08-14-sprint-01-local-state-secrets.md) | Projetos/configuração persistem; segredos usam keychain |
| 02 | [Contratos e conectividade](2026-08-14-sprint-02-driver-contracts-connectivity.md) | API modular, TLS/SSH/proxy e connection policy testados |
| 03 | [Vertical PostgreSQL/MySQL](2026-08-14-sprint-03-postgres-mysql-query-vertical.md) | Conecta, consulta, pagina e cancela nos dois bancos |
| 04 | [Shell TUI e CLI](2026-08-14-sprint-04-tui-cli-workbench-shell.md) | TUI navegável, command palette e grade streaming |
| 05 | [Editor e inteligência SQL](2026-08-14-sprint-05-sql-editor-intelligence.md) | Abas, parsing, autocomplete, parâmetros e histórico |
| 06 | [Catálogo e explorador](2026-08-14-sprint-06-catalog-explorer.md) | Introspecção completa, busca e navegação offline |
| 07 | [Visualização e edição de dados](2026-08-14-sprint-07-data-viewer-editor.md) | Grid completo e change sets seguros |
| 08 | [Engenharia DDL e permissões](2026-08-14-sprint-08-schema-ddl-security.md) | Criação/alteração de objetos, roles e grants |
| 09 | [Schema diff e migrações](2026-08-14-sprint-09-schema-diff-migrations.md) | Snapshots, diff e scripts ordenados/revisáveis |
| 10 | [Import, export e backup](2026-08-14-sprint-10-data-transfer-backup.md) | Transferência streaming e integração com ferramentas nativas |
| 11 | [Explain, diagnóstico e administração](2026-08-14-sprint-11-explain-diagnostics-admin.md) | Planos, sessões, locks, estatísticas e manutenção |
| 12 | [MCP read-only](2026-08-14-sprint-12-mcp-readonly.md) | Perfis/allowlists e tools/resources read-only por stdio |
| 13 | [MCP grants e auditoria](2026-08-14-sprint-13-mcp-grants-audit.md) | Elevação temporária, idempotência, revogação e auditoria |
| 14 | [UX, recuperação e observabilidade](2026-08-14-sprint-14-ux-recovery-observability.md) | Temas, atalhos, acessibilidade e crash recovery completos |
| 15 | [Hardening e release 1.0](2026-08-14-sprint-15-release-hardening-1-0.md) | Matriz, performance, segurança, instaladores e documentação 1.0 |

## Mapa global de crates

| Caminho | Responsabilidade | Criado em |
| --- | --- | --- |
| `crates/dexo` | Binário e composição | 00 |
| `crates/dexo-app` | Casos de uso, commands/events e policies | 00–03 |
| `crates/dexo-cli` | Contrato CLI e presenters | 00, 03–15 |
| `crates/dexo-tui` | Loop, componentes e telas | 04–14 |
| `crates/dexo-mcp` | Adapter MCP server-only | 12–13 |
| `crates/dexo-driver-api` | Tipos e traits de banco | 02 |
| `crates/dexo-driver-postgres` | PostgreSQL | 03, 06–11 |
| `crates/dexo-driver-mysql` | MySQL | 03, 06–11 |
| `crates/dexo-sql` | Documento, parser e inteligência | 05 |
| `crates/dexo-storage` | SQLite e repositories | 01, 05–06, 12–14 |
| `crates/dexo-secrets` | Keychain | 01 |
| `crates/dexo-transport` | TCP, TLS, SSH e proxy | 02 |
| `crates/dexo-runtime` | Tasks, streaming e cancelamento | 00, 03 |
| `crates/dexo-test-support` | Fakes, fixtures e containers | 00–15 |

## Cobertura da spec

| Seção da spec | Sprints responsáveis | Gate final |
| --- | --- | --- |
| 5.1 Projetos e estado local | 01, 05, 14 | recuperação e export/import sem segredo |
| 5.2 Conexões | 01–03, 15 | keychain, TLS/SSH/proxy, read-only e matriz nativa |
| 5.3 Explorador | 06 | objetos, busca, DDL, dependências e cache offline |
| 5.4 Editor SQL | 05 | parser tolerante, autocomplete, format, arquivos e snippets |
| 5.5 Execução/sessões | 03, 05 | scripts, result sets, transaction/savepoints e cancelamento |
| 5.6 Resultados | 04, 07 | grid virtual, formatos, grandes valores e edição |
| 5.7 Engenharia de schemas | 08 | forms, preview DDL, objetos, users/roles/grants |
| 5.8 Diff/migrações | 09 | snapshot, risco, dependências e script bidirecional |
| 5.9 Transferência/backup | 10 | CSV/TSV/JSON/JSONL/SQL e native tools |
| 5.10 Explain | 11 | árvore/tabela/resumo, analyze protegido e comparação |
| 5.11 Administração | 11 | sessões, locks, sizes, stats, variables e maintenance |
| 5.12 CLI híbrida | 00, 03, 06, 09–13 | todos os subcomandos, stdout/stderr e exit codes |
| 5.13 Personalização | 14 | temas, keymaps, compact mode, `NO_COLOR` |
| 5.14 MCP | 12–13 | conformidade, profiles, grants, resources/tools e audit |
| 6 Suporte por banco | 03, 06–11, 15 | versões suportadas e capability matrix |
| 7 Arquitetura | 00–03 | dependências respeitam os contratos aprovados |
| 8 Persistência | 01, 05–06, 12–14 | migrations, backups e repositories completos |
| 9 Fluxos críticos | 03, 07, 09, 12–13 | E2E por fluxo |
| 10 Segurança | 01–03, 07–08, 12–13, 15 | secret sentinels, policy e least privilege |
| 11 Erros/recuperação | 00, 03, 14 | categorias estáveis e estado desconhecido explícito |
| 12 TUI | 04, 14 | layout, compact mode e MCP area |
| 13 Stack | 00–03, 05, 12 | dependências fixadas e auditadas |
| 14 Testes | todas, 15 | unit/property/contract/E2E/conformance em CI |
| 15 Desempenho | 03–07, 10, 15 | todos os budgets medidos |
| 16 Distribuição/privacidade | 01, 14–15 | artifacts, SBOM, assinatura e zero telemetry |
| 17 Roadmap | 00–15 | todas as sprints encerradas |
| 18 Definition of Done | todas | checklist 1.0 da Sprint 15 |
| 19 Riscos | 02–03, 07, 09, 12–13, 15 | cada mitigação possui teste automatizado |

## Critério de conclusão global

Dexo 1.0 está completo somente quando a Sprint 15 comprovar todos os itens da matriz acima, não houver checkbox pendente em nenhum plano, a suíte integral passar nos três sistemas operacionais e os artefatos assinados reproduzíveis forem publicados a partir de um tag limpo.
