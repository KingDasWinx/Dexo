# Auditoria do Command Palette do Dexo

**Data:** 2026-08-16  
**Branch/HEAD:** `development` / `dc2c5cbf298b46b2645ca98980d66a3ec827739a`  
**Escopo principal:** fluxo completo `palette -> Action -> update -> Effect/runtime -> tela`, com revisão individual das 129 entradas registradas.  
**Método:** Codebase Memory em modo Auditor (grafo completo, busca, traces inbound/outbound, snippets e cobertura), leitura direta dos fontes apontados, comparação com planos/checklist e execução de testes.

## Resumo executivo

O Command Palette não é apenas inconsistente: ele expõe comandos que pulam a etapa de entrada, operam sobre seleção invisível, abrem telas sem dados/teclado, chamam implementações parciais ou executam outra operação. O caso relatado de projeto novo é confirmado: `project.create` constrói `CreateProject { name: String::new() }`, fecha o palette e envia o nome vazio ao storage, embora a tela de projetos já tenha `ProjectsMode::Create` e `name_input`.

O problema sistêmico é a falta de um contrato único de comando. Hoje cada entrada contém uma closure `fn() -> Action`, que não consegue pedir argumentos nem preparar o fluxo da tela. Apenas 11 das 129 entradas calculam `disabled_reason`; as outras 118 aparecem habilitadas em qualquer contexto. A cobertura de testes confirma IDs e handlers isolados, mas não seleciona comandos pelo palette e não valida a experiência renderizada.

**Classificação das 129 entradas:** 46 adequadas, 45 dependentes de contexto sem guarda suficiente, 11 parciais/enganosas e 27 quebradas.

### Achados mais graves

1. **Crítico — Import/backup/restore podem sobrescrever o arquivo escolhido com um CSV de resultados.** `transfer.import`, `backup.dump` e `backup.restore` abrem a mesma tela; `run_transfer` ignora `mode` e sempre chama `export_rows`. Além de não importar/restaurar, o caminho selecionado pode virar destino de exportação.
2. **Alto — CRUD de projetos pula as telas de entrada/confirmação.** Create e rename enviam nome vazio; switch usa seleção invisível; delete cria confirmação que não é renderizada porque `projects.open` continua falso.
3. **Alto — Manage Grants abre uma tela vazia e sem rota de teclado para fechar/aplicar.** `SecurityScreen::create_role` e `grant_select` não possuem chamadores; `handle_key` não trata `security.open`.
4. **Alto — Compare Schema abre estado default vazio.** `OpenSchemaDiff` apenas define `open = true`; `SchemaManager::diff` e `SchemaDiffScreen::from_ordered` não têm chamadores no grafo.
5. **Alto — Revoke All MCP Grants não revoga “todos”.** A ação usa somente `mcp_profiles.name`; o loader carrega apenas o primeiro perfil. Sem perfil carregado, tenta revogar nome vazio. Não há confirmação no caminho do palette.
6. **Alto — Export Diagnostics não exporta.** Ele monta e mostra preview; `DiagnosticBundle::write_zip` só é chamado por testes e o overlay não tem ação de fechamento.
7. **Alto — Submit Parameters pode executar a query.** Fora de um prompt de parâmetros, a ação limpa o estado e chama `start_query`, funcionando como execução inesperada.
8. **Médio — Insert Snippet é inerte em uso real.** Existe consumidor de `Effect::LoadSnippets`, mas nenhuma produção desse efeito; a lista começa vazia e o comando retorna imediatamente.
9. **Médio — Explain ignora o cursor real.** O runtime chama `run_live(document, 0, ...)`, portanto escolhe a primeira instrução, enquanto o teste que usa o cursor testa `ExplainManager` isoladamente.
10. **Médio — O palette perde o foco anterior.** `close_palette` sempre muda `Focus::Palette` para `Focus::Editor`, mesmo quando foi aberto no Explorer, Results ou Inspector.
11. **Médio — O campo de shortcut é código morto.** As 129 entradas mantêm atalhos estáticos, mas `render_palette` nunca lê `entry.shortcut`; ainda assim o checklist afirma “Palette with shortcuts: pass”.
12. **Médio — A suíte não está verde.** `cargo test -p dexo-tui` terminou com 3 snapshots divergentes; os 6 testes unitários de `palette` passam porque não cobrem a seleção real de nenhum comando.

## Auditoria das funções do mecanismo do palette

| Função | Estado | Avaliação |
|---|---|---|
| `palette_entries` | ❌ Corrigir | Registro monolítico de 1.091 linhas; closures sem contexto/argumentos causam nomes vazios e ações sem preparação de tela. Mistura disponibilidade, apresentação e construção da ação. |
| `results_menu_items` | ⚠️ Revisar | Lista paralela com IDs sintéticos e IDs do palette. Funciona, mas duplica o registro e herda comandos contextualmente inválidos como `data.filter`. |
| `action_by_id` | ❌ Corrigir | Reconstrói o registro com `Model::default()`. Isso elimina o estado real e torna impossível construir comandos contextuais ou parametrizados corretamente. |
| `popup_list_rows` | ✅ Adequada | Cálculo limitado e consistente com a altura renderizada. |
| `scroll_to_selection` | ✅ Adequada | Trata lista vazia, janela e saturação; possui teste útil. |
| `filter_entries` | ✅ Adequada | Filtro/ordenação determinísticos. |
| `score` | ✅ Adequada | Normaliza query e pesquisa título, keywords e ID. |
| `score_text` | ✅ Adequada | Prefixo, início de palavra e subsequência têm prioridade coerente. |
| `is_subsequence` | ✅ Adequada | Implementação simples e correta para o contrato atual. |
| `render_palette` | ❌ Corrigir | Não renderiza `shortcut`; não oferece detalhe/feedback acionável além do texto inline; depende de flags de disponibilidade quase sempre ausentes. |
| `open_palette` | ⚠️ Revisar | Abre/reset corretamente, mas não guarda o foco/origem. |
| `handle_palette_key` | ✅ Base adequada | Busca, navegação e seleção funcionam; faltam testes end-to-end e feedback quando Enter cai em item desabilitado. |
| `move_palette_selection` | ✅ Adequada | Usa contagem filtrada e scroll compartilhado corretamente. |
| `close_palette` | ❌ Corrigir | Força sempre `Focus::Editor`; precisa restaurar o foco anterior ou deixar a ação escolhida decidir. |
| `palette_select` | ❌ Corrigir | Fecha antes de preparar fluxos, só suporta `fn() -> Action` e retorna silenciosamente em comando desabilitado. Precisa despachar um comando contextual que possa abrir prompt/tela. |

## Matriz das 129 entradas

Legenda: **✅ adequada** = comportamento anunciado está ligado; **⚠️ contexto** = handler funciona, mas o palette não representa pré-condições e pode gerar no-op/estado invisível; **🟠 parcial** = comportamento incompleto ou título enganoso; **❌ quebrada** = fluxo determinístico incorreto, inacessível ou operação diferente da anunciada.

### Workbench, query e transação

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 1 | `workbench.quit` | ✅ | Faz checkpoint, persiste layout e encerra. |
| 2 | `palette.open` | ✅ | Reabre/reset o próprio palette corretamente. |
| 3 | `query.execute` | 🟠 | Duplica `query.execute_document`; não exige sessão ativa, então cria tabs/operação antes de falhar com “session is closed”. |
| 4 | `query.execute_statement` | ⚠️ | Seleção da instrução funciona, mas falta disponibilidade por sessão. |
| 5 | `query.execute_selection` | ⚠️ | Verifica seleção, mas não sessão ativa. |
| 6 | `query.execute_document` | ⚠️ | Verifica editor vazio, mas não sessão ativa. |
| 7 | `query.cancel` | ✅ | Disponibilidade usa `active_query` e o cancelamento é correlacionado. |
| 8 | `transaction.begin` | ⚠️ | Exige sessão/Idle, mas continua habilitado em conexão read-only. |
| 9 | `transaction.savepoint` | 🟠 | Sempre cria o nome fixo `sp1`; não existe entrada/seleção de savepoint. |
| 10 | `transaction.rollback_savepoint` | ❌ | Sempre usa `sp1` e o palette bloqueia estado `Failed`, embora o handler aceite `Failed`. |
| 11 | `transaction.release_savepoint` | 🟠 | Sempre libera `sp1`; não permite escolher qual savepoint. |
| 12 | `transaction.commit` | ✅ | Disponibilidade e effect para sessão ativa são coerentes. |
| 13 | `transaction.rollback` | ✅ | Disponível em Active/Failed e emite rollback para a sessão ativa. |

### Ajuda, foco e layout

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 14 | `help.open` | ✅ | Abre/fecha ajuda e possui teste renderizado. |
| 15 | `focus.explorer` | ✅ | Torna o painel visível e muda foco. |
| 16 | `focus.editor` | ✅ | Muda foco para editor. |
| 17 | `focus.results` | ✅ | Torna resultados visíveis e muda foco. |
| 18 | `focus.inspector` | ✅ | Torna inspetor visível e muda foco. |
| 19 | `layout.cycle` | ✅ | Aplica o próximo preset. |
| 20 | `layout.results_focus` | ✅ | Aplica ResultsWide e foco em Results. |
| 21 | `layout.hide_inspector` | ✅ | Alterna visibilidade, corrige foco e recalcula viewport. |
| 22 | `layout.reset` | ✅ | Restaura preset Normal. |
| 23 | `layout.results_grow` | ✅ | Ajusta altura com clamp. |
| 24 | `layout.results_shrink` | ✅ | Ajusta altura com clamp. |
| 25 | `layout.explorer_grow` | ✅ | Ajusta largura com clamp. |
| 26 | `layout.explorer_shrink` | ✅ | Ajusta largura com clamp. |
| 27 | `layout.inspector_grow` | ✅ | Ajusta largura com clamp. |
| 28 | `layout.inspector_shrink` | ✅ | Ajusta largura com clamp. |

### Dados e resultados derivados

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 29 | `data.copy.csv` | ⚠️ | Cópia funciona, mas item fica habilitado sem grid/seleção. |
| 30 | `data.copy.text` | ⚠️ | Mesmo problema de disponibilidade. |
| 31 | `data.copy.json` | ⚠️ | Mesmo problema de disponibilidade. |
| 32 | `data.copy.markdown` | ⚠️ | Mesmo problema de disponibilidade. |
| 33 | `data.copy.sql` | ⚠️ | Mesmo problema de disponibilidade; depende também de dialeto/contexto. |
| 34 | `data.apply` | ⚠️ | Handler protege read-only/produção/sessão, mas o palette não exige mudanças pendentes. |
| 35 | `data.revert` | ⚠️ | Reverte corretamente, mas fica habilitado sem mudanças. |
| 36 | `data.nav_back` | ⚠️ | Funciona com breadcrumbs; sem eles retorna silenciosamente. |
| 37 | `data.page_next` | ❌ | Sem sessão/target, altera offset e deixa `loading = true` sem produzir effect. |
| 38 | `data.page_prev` | ❌ | Mesmo vazamento de estado; pode aparentar carregamento infinito. |
| 39 | `data.sort` | ❌ | Não existe produção de `model.data.sort`; o comando apenas reexecuta o estado atual. |
| 40 | `data.filter` | 🟠 | Não abre editor de filtro; só reaplica filtro existente (normalmente criado por FK). |
| 41 | `data.review` | ⚠️ | Pode abrir review com zero operações e alvo vazio. |
| 42 | `data.related` | ⚠️ | Funciona com FK/linha preparada; caso contrário é no-op sem motivo no palette. |
| 43 | `data.inspect` | ⚠️ | Funciona com célula selecionada; caso contrário é no-op. |

### Schema, transferência, explain, admin e MCP

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 44 | `schema.preview` | 🟠 | Opera sobre `schema_editor` oculto; não abre a aba DDL nem prepara o formulário. Validação pode falhar sem feedback visível. |
| 45 | `schema.raw` | 🟠 | “Apply Raw DDL” não aplica: só grava `raw_sql` e `form_diff`. O título é incorreto. |
| 46 | `schema.diff` | ❌ | Abre `SchemaDiffScreen::default()` vazio; o manager real de diff não está ligado ao fluxo. |
| 47 | `transfer.export` | 🟠 | Exporta só os rows materializados, sincronicamente no update loop; sem streaming/cancelamento e sem guard de resultados. |
| 48 | `transfer.import` | ❌ | Abre a tela em modo export e `run_transfer` sempre chama `export_rows`; pode sobrescrever o arquivo de importação. |
| 49 | `backup.dump` | ❌ | Define mode “backup”, mas `run_transfer` ignora o mode e exporta CSV. |
| 50 | `backup.restore` | ❌ | Define mode “restore”, mas exporta CSV para o caminho escolhido; risco de sobrescrever backup. |
| 51 | `schema.security` | ❌ | Abre tela vazia, sem carregamento, edição/aplicação ou handler de Esc. Métodos de criação/grant não têm chamadores. |
| 52 | `explain.open` | 🟠 | Usa documento com cursor `0`, portanto explica a primeira instrução, não a instrução sob o cursor; sem sessão vira no-op. |
| 53 | `admin.sessions` | ⚠️ | Funciona com sessão ativa; sem sessão abre overlay vazio. |
| 54 | `mcp.profiles` | 🟠 | Loader pega somente o primeiro perfil e o teclado só trata Esc/revoke; não há navegação/edição completa. |

### Explorer

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 55 | `explorer.expand` | ⚠️ | Depende de nó selecionado; caso contrário retorna silenciosamente. |
| 56 | `explorer.refresh` | ❌ | Sem seleção/sessão pode limpar o explorer e não emitir reload. |
| 57 | `explorer.refresh_all` | ❌ | Limpa antes de verificar sessão; no modo offline pode apagar a árvore visível sem recarregar. |
| 58 | `explorer.inspect` | ⚠️ | Exige nó e sessão; ambos ausentes viram no-op sem motivo. |
| 59 | `explorer.ddl` | ⚠️ | Mesmo requisito; altera tab mesmo quando o inspector não abre. |
| 60 | `explorer.refresh_subtree` | ❌ | É tratado junto com `RefreshCatalogAll` e recarrega tudo, não a subtree anunciada. |
| 61 | `explorer.up` | ✅ | Move seleção e sincroniza scroll. |
| 62 | `explorer.down` | ✅ | Move seleção e sincroniza scroll. |
| 63 | `explorer.dependencies` | ⚠️ | Exige nó/sessão; abre a aba combinada de relações. |
| 64 | `explorer.dependents` | ⚠️ | Exige nó/sessão; abre a mesma aba combinada e apenas adiciona mensagem “dependents”. |

### Abas, documentos e seleção de resultados

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 65 | `tab.sql` | ✅ | Seleciona índice com bounds check. |
| 66 | `tab.data` | ✅ | Seleciona índice com bounds check. |
| 67 | `tab.ddl` | ✅ | Seleciona índice com bounds check. |
| 68 | `tab.properties` | ✅ | Seleciona índice com bounds check. |
| 69 | `tab.explain` | ✅ | Seleciona índice com bounds check. |
| 70 | `tab.next` | ✅ | Cicla com segurança. |
| 71 | `document.next` | ✅ | Cicla documentos com segurança. |
| 72 | `document.new` | ✅ | Cria scratch e o ativa. |
| 73 | `document.save` | ✅ | Salva ou abre file picker quando não há path. |
| 74 | `document.open` | ✅ | Abre file picker em modo Open. |
| 75 | `results.select_row` | ⚠️ | Funciona com cursor; sem resultados é no-op. |
| 76 | `results.select_column` | ⚠️ | Funciona com cursor; sem resultados é no-op. |
| 77 | `results.next_tab` | ⚠️ | Funciona com tabs; sem resultados é no-op. |
| 78 | `results.prev_tab` | ⚠️ | Funciona com tabs; sem resultados é no-op. |
| 79 | `inspector.next_tab` | ⚠️ | Altera estado mesmo sem inspector aberto; efeito fica invisível. |

### Preferências, explorer avançado e navegação no grid

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 80 | `settings.theme` | ✅ | Alterna, aplica e persiste tema. |
| 81 | `settings.keymap` | ✅ | Alterna e persiste keymap. |
| 82 | `settings.mouse` | ✅ | Alterna e persiste mouse. |
| 83 | `explorer.data` | ⚠️ | Exige tabela selecionada e sessão; só a falta de sessão gera mensagem. |
| 84 | `editor.goto` | ✅ | Procura definição e fornece “no definition at cursor” quando não encontra. |
| 85 | `explorer.copy_name` | ⚠️ | Exige nó selecionado; senão no-op. |
| 86 | `explorer.copy_simple` | ⚠️ | Exige nó selecionado; senão no-op. |
| 87 | `explorer.copy_ddl` | ⚠️ | Exige DDL previamente carregado; handler dá mensagem, mas palette não explica. |
| 88 | `explorer.favorite` | ⚠️ | Exige nó/projeto/conexão; faltas podem virar no-op. |
| 89 | `explorer.favorites_only` | ✅ | Alterna filtro local. |
| 90 | `explorer.system_objects` | ⚠️ | Alterna localmente; refresh só ocorre conectado, deixando estado offline potencialmente incoerente. |
| 91 | `results.up` | ⚠️ | Funciona com grid; sem rows é no-op. |
| 92 | `results.down` | ⚠️ | Funciona com grid; sem rows é no-op. |
| 93 | `results.left` | ⚠️ | Funciona com colunas; sem grid é no-op. |
| 94 | `results.right` | ⚠️ | Funciona com colunas; sem grid é no-op. |
| 95 | `results.pageup` | ⚠️ | Funciona com viewport; sem grid é no-op. |
| 96 | `results.pagedown` | ⚠️ | Funciona com viewport; sem grid é no-op. |
| 97 | `results.top` | ⚠️ | Funciona com seleção; sem grid não há resultado visível. |
| 98 | `results.extend_up` | ⚠️ | Exige seleção/grid. |
| 99 | `results.extend_down` | ⚠️ | Exige seleção/grid. |
| 100 | `results.actions` | ⚠️ | Só abre menu se houver row; palette o mostra sempre habilitado. |
| 101 | `results.toggle_pick` | ⚠️ | Exige row/cursor; caso contrário é no-op. |

### Conexões e projetos

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 102 | `connection.add` | ✅ | Abre formulário visível e editável. |
| 103 | `connection.browse` | ✅ | Abre browser de conexões. |
| 104 | `connection.connect` | ❌ | Conecta a seleção interna invisível; não abre browser para o usuário escolher. |
| 105 | `connection.duplicate` | ❌ | Duplica a seleção interna invisível ou retorna no-op. |
| 106 | `connection.test` | ❌ | Testa a seleção interna invisível ou retorna no-op. |
| 107 | `connection.delete` | ❌ | Define prompt em `delete_target`, mas não abre o browser; confirmação fica invisível. |
| 108 | `connection.close_session` | ❌ | Fecha seleção interna/active session sem mostrar qual sessão será afetada. |
| 109 | `project.browse` | ✅ | Abre a tela e carrega projetos. |
| 110 | `project.switch` | ❌ | Envia nome vazio e usa a seleção interna; não abre chooser e pode apenas recarregar lista. |
| 111 | `project.create` | ❌ | Envia nome vazio direto ao storage; não abre `ProjectsMode::Create`. |
| 112 | `project.rename` | ❌ | Envia nome vazio para a seleção interna; não abre `ProjectsMode::Rename`. |
| 113 | `project.delete` | ❌ | Gera preview para seleção invisível; a confirmação não é renderizada porque `projects.open` permanece falso. |

### Configuração, recovery, MCP, editor e diagnósticos

| # | ID | Estado | Resultado da revisão |
|---:|---|---|---|
| 114 | `config.transfer` | ✅ | Abre overlay com modo, path, preview e effects de import/export. |
| 115 | `settings.open` | ✅ | Abre tela de settings e sincroniza mouse. |
| 116 | `settings.reset` | ❌ | Primeira seleção só liga confirmação oculta; segunda pode resetar sem tela/feedback adequado. |
| 117 | `recovery.open` | ✅ | Abre overlay de recovery. |
| 118 | `recovery.restore` | ⚠️ | Recupera diretamente se houver checkpoints; não mostra pré-condição/preview pelo palette. |
| 119 | `recovery.discard` | ❌ | Primeira seleção liga confirmação oculta; segunda descarta. O fluxo visível só existe dentro do overlay. |
| 120 | `mcp.audit` | ✅ | Abre e carrega auditoria. |
| 121 | `mcp.revoke_all` | ❌ | Revoga apenas `mcp_profiles.name` (primeiro perfil ou vazio), sem confirmação; não corresponde a “all”. |
| 122 | `editor.complete` | ✅ | Atualiza inteligência e abre completions. |
| 123 | `editor.format` | 🟠 | Formata sempre como PostgreSQL, ignorando conexão/dialeto MySQL. |
| 124 | `editor.accept_completion` | ⚠️ | Funciona com completion carregada; sem ela é no-op. |
| 125 | `editor.snippet` | ❌ | Snippets começam vazios e `Effect::LoadSnippets` nunca é produzido no fluxo real. |
| 126 | `editor.parameters` | ❌ | Fora do prompt pode disparar `start_query`; comando de submissão vira execução inesperada. |
| 127 | `editor.history` | ✅ | Abre overlay e carrega histórico. |
| 128 | `editor.history.clear` | 🟠 | Apaga imediatamente só o histórico da conexão atual, enquanto a busca carrega `connection_id: None`; título/escopo e confirmação são insuficientes. |
| 129 | `diagnostics.export` | ❌ | Apenas monta preview e o deixa sem rota de fechamento; nunca chama `write_zip` nem escolhe destino. |

## Problemas de integração fora do arquivo do palette

### Capacidades implementadas, mas desconectadas

- `SchemaManager::diff`: zero chamadores; a TUI abre um estado vazio em vez de usar o manager.
- `TransferManager::export_batches`: zero chamadores; a TUI exporta sincronicamente com `rows().to_vec()`.
- `SecurityScreen::create_role` e `grant_select`: zero chamadores.
- `DiagnosticBundle::write_zip`: somente dois chamadores, ambos testes.
- `SchemaDiffScreen::from_ordered` e `TransferScreen::from_detection`: zero chamadores.
- `Effect::LoadSnippets`: existe no dispatcher, mas não há produtor no fluxo da aplicação.

Isso mostra que os crates de domínio não são o principal problema. A camada de orquestração da TUI foi considerada “pronta” com base em testes de unidade dos componentes.

### Testes que dão falsa segurança

- `every_current_action_is_in_palette` verifica somente 12 IDs escolhidos, não as 129 entradas nem seus efeitos.
- `action_registry_every_command_is_palette_reachable` verifica somente 11 IDs.
- `every_registered_command_is_palette_reachable` garante que comandos com keybinding têm um ID, não que possam ser executados no contexto correto.
- Não há teste usando `Action::PaletteSelect` ou simulando `OpenPalette -> digitar busca -> Enter`.
- Os testes de project CRUD chamam storage/handlers diretamente; não detectam o nome vazio produzido pelo palette.
- O teste de explain usa `ExplainManager` diretamente com cursor correto; o dispatcher real passa cursor `0`.
- Os testes de transfer verificam exportador/manager isolado e snapshots com fixtures; não detectam que import/backup/restore chamam export.
- O checklist de release está marcado integralmente como pass, mas `cargo test -p dexo-tui` falha em 3 snapshots.

## Verificação executada

- CodeGraph completo: 6.347 nós, 31.486 relações, branch `development`, HEAD `dc2c5cb`.
- `search_graph` paginado para o escopo do palette: 15 funções encontradas, sem página restante.
- Traces inbound/outbound para registro, filtro, seleção, render, project/transfer/schema/security/explain/diagnostics e managers desconectados.
- Leitura direta de `palette.rs`, `update.rs`, `render.rs`, `action.rs`, `model.rs`, keymaps, screens, runtime e storage relevantes.
- `cargo test -p dexo-tui --lib palette::tests`: **6 passed**.
- `cargo test -p dexo-tui`: testes funcionais passam até snapshots; **3 failures** em `snapshot_review_and_related_tab`, `snapshot_schema_editor_full` e `snapshot_schema_editor_compact_and_preview`.
- Os `.snap.new` gerados pela auditoria foram removidos; nenhuma alteração de código foi feita.

## Limitações declaradas

Esta é uma auditoria exaustiva e verificável do Command Palette e dos fluxos diretamente acionados por ele, não uma prova formal de correção das 6.347 entidades do repositório. O índice foi reconstruído em modo full e não registra gaps nos arquivos do escopo. `check_index_coverage` continuou reportando `metadata_changed` mesmo após o reindex; por isso as conclusões materiais foram confirmadas por leitura/grep direto dos fontes. A única faixa `parse_partial` mostrada por `index_status` fica em `scripts/verify-release.ps1`, fora deste escopo.

## Direção de correção recomendada

O conserto deve trocar closures `fn() -> Action` por comandos com contrato explícito: disponibilidade, motivo, modo de invocação e handler contextual. Comandos que precisam de entrada devem abrir a tela/modal dona do estado; comandos sobre seleção devem capturar/mostrar o alvo; operações destrutivas devem entrar no mesmo fluxo de preview/confirmação usado pela UI normal. Depois, cada ID precisa de um teste de contrato e os fluxos críticos precisam de testes renderizados pelo caminho real do palette.
