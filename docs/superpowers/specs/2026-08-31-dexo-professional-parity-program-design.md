# Dexo Professional Parity Program — Master Design

**Status:** aprovado como especificação-mãe pós-workspace

**Data:** 2026-08-31

**Escopo principal:** PostgreSQL e MySQL

**Documento-base obrigatório:**
[`2026-08-31-workspace-database-first-design.md`](./2026-08-31-workspace-database-first-design.md)

## 1. Papel deste documento

Esta é a especificação de produto e arquitetura de longo prazo do Dexo após a
implantação do workspace database-first. Ela descreve o conjunto completo de
capacidades necessárias para transformar o Dexo em um database workbench
terminal-first com paridade profissional de tarefas em relação a DataGrip e DBeaver.

Este documento não é um plano de implementação único. Ele é a fonte normativa para
várias specs e planos menores. Todo plano derivado deve:

1. citar os IDs de requisito atendidos;
2. respeitar as dependências e invariantes deste documento;
3. definir os arquivos e contratos concretos afetados;
4. incluir verificação proporcional ao fluxo vertical;
5. não marcar um requisito como completo apenas porque um motor interno existe;
6. atualizar a matriz de cobertura quando o requisito for entregue.

A ordem de implementação pode evoluir. O comportamento, as fronteiras de segurança e
os critérios de aceitação definidos aqui permanecem estáveis até uma revisão explícita
desta spec.

## 2. Premissa obrigatória

Esta spec assume como concluída a fundação database-first:

- projeto como limite do workspace;
- conexões de projeto e conexões compartilhadas;
- navigator persistente;
- Start Center sem SQL obrigatório;
- abas tipadas com estado isolado;
- catálogo cache-first e conexão sob demanda;
- restauração de projeto, abas, foco e layout;
- abertura direta de tabela em `TableData`;
- ações de domínio comuns a teclado, mouse e palette.

Se a fundação ainda não estiver concluída, planos derivados desta spec só podem
implementar contratos internos que não criem dependência na arquitetura antiga. A UI
profissional descrita aqui não deve ser encaixada novamente em estados globais de SQL,
data ou inspector.

## 3. Definição de paridade profissional

Paridade profissional significa equivalência de tarefa, segurança e previsibilidade,
não cópia visual de uma aplicação desktop.

O Dexo atinge paridade profissional quando um usuário consegue, sem sair do terminal:

- organizar projetos e databases;
- navegar por catálogo e objetos;
- visualizar, filtrar, editar e transferir dados;
- criar, alterar, comparar e inspecionar schemas;
- escrever e executar SQL quando desejar;
- investigar sessões, locks, planos e desempenho;
- salvar e repetir tarefas operacionais;
- usar teclado, mouse ou command palette sobre os mesmos comandos;
- compreender antes de confirmar qualquer mudança;
- recuperar-se de falhas sem perder estado local ou repetir mutações;
- automatizar via CLI e MCP dentro de políticas explícitas.

Não faz parte da definição de paridade:

- reproduzir pixel a pixel DataGrip ou DBeaver;
- embutir uma GUI, navegador web ou runtime Electron;
- suportar superficialmente dezenas de bancos;
- cloud sync, conta obrigatória ou colaboração em tempo real;
- marketplace de plugins ou ABI de driver de terceiros nesta geração;
- editor visual de diagramas dependente de drag-and-drop;
- embutir um modelo de IA ou chat proprietário.

### 3.1 Referências oficiais de paridade

As referências abaixo foram verificadas em 2026-08-31. Elas definem tarefas de
comparação, não uma obrigação de copiar layout, tecnologia, licenciamento ou detalhes
exclusivos de edição comercial:

- DataGrip [Data editor and viewer](https://www.jetbrains.com/help/datagrip/data-editor-and-viewer.html)
  e [Rows](https://www.jetbrains.com/help/datagrip/rows.html): edição de célula,
  insert, clone, delete, submit/revert, filtros, ordenação, paginação, record view,
  relações e extractors.
- DataGrip [Database Explorer](https://www.jetbrains.com/help/datagrip/database-explorer.html):
  fontes, árvore de objetos, introspecção, ações contextuais e abertura direta de
  dados/objetos.
- DataGrip [Database diagrams](https://www.jetbrains.com/help/datagrip/creating-diagrams.html):
  relações por data source, schema e tabela, persistência e exportação.
- DBeaver [Data Editor](https://dbeaver.com/docs/dbeaver/Data-Editor/): grid,
  edição, filtros, value panel, refresh, export e geração de SQL.
- DBeaver [Connections](https://dbeaver.com/docs/dbeaver/Database-Navigator/),
  [Metadata search](https://dbeaver.com/docs/dbeaver/DB-Metadata-Search/) e
  [Object editor](https://dbeaver.com/docs/dbeaver/Database-Object-Editor/): navegação,
  busca entre conexões, propriedades, dados, DDL e diagramas.
- DBeaver [Data transfer](https://dbeaver.com/docs/dbeaver/Data-transfer/) e
  [Data compare](https://dbeaver.com/docs/dbeaver/Data-compare/): import/export,
  table-to-table, mapping, comparação cross-database e geração de sync.
- DBeaver [Task management](https://dbeaver.com/docs/dbeaver/Task-Management/) e
  [Background tasks](https://dbeaver.com/docs/dbeaver/Background-Tasks/): operações
  reutilizáveis, execução por CLI, progresso, cancelamento e scheduling externo.
- DBeaver [Diagrams](https://dbeaver.com/docs/dbeaver/ER-Diagrams/): relações,
  schemas, layouts persistentes, busca e exportação.

Funcionalidade citada por uma referência continua sujeita às decisões terminal-first,
às políticas de segurança e aos gates desta spec.

## 4. Diagnóstico de partida

O Dexo já possui uma arquitetura modular com drivers, serviços de aplicação, storage
local, TUI, CLI e MCP. Também já possui motores para várias funções avançadas.
Entretanto, a presença de um motor não garante um fluxo vertical utilizável.

### 4.1 Capacidades existentes

- PostgreSQL e MySQL, TLS, SSH, proxies e keychain.
- Múltiplas sessões e transações.
- Catálogo lazy, cache offline, favoritos, DDL e dependências.
- Editor SQL, histórico, snippets, parâmetros e completion.
- Resultados paginados e virtualizados, seleção e cópia em vários formatos.
- Contratos de insert, update e delete.
- Change set local, preview e aplicação transacional.
- Schema editor, schema diff, transfer, backup/restore e explain.
- Administração, recovery, settings, diagnostics e MCP governado.

### 4.2 Lacunas estruturais confirmadas

- Metadata de chave, unique, default e geração não chega ao fluxo editável do grid.
- A TUI não cria updates, inserts ou deletes a partir da interação do usuário.
- Testes de mutação injetam metadata e change sets manualmente.
- Vários estados de data e objeto ainda são globais na arquitetura anterior.
- Recursos existentes carecem de action surfaces, feedback e testes verticais.
- Compare de dados, diagramas, tasks, query manager completo e object editor profundo
  ainda não formam produtos completos.

## 5. Convenções normativas

### 5.1 Palavras

- **DEVE:** requisito obrigatório.
- **NÃO DEVE:** comportamento proibido.
- **PODE:** capacidade opcional ou dependente do driver.
- **FUTURO:** não bloqueia paridade PostgreSQL/MySQL, mas possui gate explícito.

### 5.2 Prioridades

- **P0:** necessário para um fluxo diário seguro e completo.
- **P1:** necessário para paridade profissional ampla.
- **P2:** capacidade avançada que diferencia o produto ou automatiza trabalho.
- **P3:** expansão futura após estabilização dos contratos.

### 5.3 Estado atual

- **E:** motor existente, fluxo vertical ausente ou incompleto.
- **P:** implementação parcial e alcançável.
- **A:** ausente.
- **F:** futuro condicionado.

### 5.4 Formato dos requisitos

As tabelas usam:

| Campo | Significado |
| --- | --- |
| ID | Identificador estável para specs, planos, testes e changelog |
| Pri | Prioridade |
| Atual | Estado atual |
| Requisito | Comportamento normativo |
| Aceitação | Evidência observável mínima |
| Depende | IDs ou gates obrigatórios |

## 6. Arquitetura do programa

```text
PostgreSQL / MySQL
        |
        v
Driver capability contracts
        |
        v
dexo-app domain services
        |
        +-- storage / recovery / audit
        +-- task runtime / cancellation
        `-- policy / risk / confirmation
        |
        v
Workspace tabs and navigator actions
        |
        +-- TUI
        +-- CLI
        `-- MCP governed exposure
```

### 6.1 Regra de completude vertical

Um requisito dependente do servidor só é completo quando possui:

1. contrato de driver ou declaração explícita de capacidade;
2. implementação PostgreSQL e MySQL aplicável;
3. serviço de aplicação independente de TUI;
4. action/effect ou interface equivalente no adapter;
5. feedback de loading, sucesso, erro e cancelamento;
6. política de segurança aplicável;
7. teste vertical com banco real no CI de integração.

### 6.2 Contratos centrais

- `TableEditMetadataProvider`: metadata necessária para editabilidade.
- `EditableDataSource`: origem, identidade e capacidades de mutação.
- `ChangeSetService`: mudanças locais, undo/redo, preview e apply.
- `BulkMutationPlan`: seleção, transformação, limites e atomicidade.
- `ObjectMetadataProvider`: propriedades e subobjetos completos.
- `ObjectMutationPlanner`: DDL tipado, risco e preview.
- `SqlGenerator`: geração segura e específica por driver.
- `DataComparisonProvider`: mapeamento, diff e sync.
- `BackgroundTask`: estado, progresso, cancelamento, retry e logs.
- `DriverCapabilityDescriptor`: disponibilidade e motivo estruturado.

## 7. Invariantes globais

1. Nenhuma ação mutável é aplicada automaticamente ao editar a UI.
2. Nenhuma reconexão repete query mutável, DDL, import ou mutação.
3. Toda ação destrutiva possui preview e alvo visível.
4. Produção e read-only são visíveis e aplicados por política central.
5. Segredos nunca entram em SQLite, logs, argv, clipboard ou diagnóstico.
6. Erros preservam mudanças locais revisáveis.
7. Fechar aba, trocar projeto, refresh ou paginação não descartam mudanças sem
   decisão explícita.
8. Teclado, mouse e palette convergem nas mesmas ações de domínio.
9. Limitações de driver aparecem como capacidade indisponível com razão.
10. O primeiro frame não depende da rede.
11. Resultados, transferências e comparações grandes usam streaming e backpressure.
12. Operações longas são canceláveis e não bloqueiam render/input.

## 8. META — Metadata e editabilidade

### 8.1 Modelo

`TableEditMetadata` deve conter, no mínimo:

```text
object identity
object kind and updatability
columns in server order
native and logical types
nullable and default expression
generated / identity / auto-increment
primary key and unique constraints
foreign keys in both directions
insert/update/delete capability
read-only reason
server/version capability context
```

### 8.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| META-001 | P0 | A | Definir `TableEditMetadata` neutro de adapter | TUI, CLI e MCP conseguem consumir o mesmo modelo | BASE |
| META-002 | P0 | A | Expor provider de metadata editável no contrato de sessão | Sessão declara disponibilidade e razão quando ausente | META-001 |
| META-003 | P0 | A | Carregar nome, ordem e tipo nativo de todas as colunas | Grid recebe metadata idêntica à ordem do fetch | META-002 |
| META-004 | P0 | A | Carregar nullable e default/expression | Insert distingue omitido, default, null e valor | META-003 |
| META-005 | P0 | A | Marcar generated, identity e auto-increment | UI bloqueia edição indevida e omite coluna no insert | META-003 |
| META-006 | P0 | A | Carregar primary key simples ou composta | Cada linha recebe identidade estável completa | META-002 |
| META-007 | P0 | A | Carregar unique constraints e nulabilidade | Fallback unique só aceita combinação confiável | META-002 |
| META-008 | P0 | A | Determinar capacidade insert/update/delete por objeto | View read-only explica a restrição específica | META-002 |
| META-009 | P0 | A | Retornar motivo estruturado de read-only | TUI mostra razão e ação possível | META-008 |
| META-010 | P0 | A | Suportar identificador manual por tabela | Usuário escolhe colunas antes de habilitar edição | META-003 |
| META-011 | P0 | A | Persistir identificador manual por projeto/conexão/objeto | Reabertura restaura a escolha e permite revogá-la | META-010 |
| META-012 | P0 | A | Invalidar identidade manual após mudança incompatível de schema | Edição volta a read-only até nova escolha | META-011 |
| META-013 | P1 | P | Carregar foreign keys de saída | Linha navega para registros referenciados | META-002 |
| META-014 | P1 | A | Carregar foreign keys de entrada | Linha encontra registros que a referenciam | META-002 |
| META-015 | P1 | A | Expor enums/domínios/conjuntos de valores quando barato | Editor oferece escolha tipada sem consulta arbitrária | META-003 |
| META-016 | P1 | A | Expor precisão, escala, tamanho, charset e collation | Validação/formatação respeita metadata real | META-003 |
| META-017 | P1 | A | Cachear metadata com versão e timestamp | Aba offline explica idade e origem do dado | BASE |
| META-018 | P1 | A | Invalidar cache após DDL conhecido | Próxima operação não usa schema obsoleto | META-017 |
| META-019 | P1 | A | Detectar source editável de query simples | SELECT direto de uma tabela pode declarar origem | META-002 |
| META-020 | P1 | A | Tratar joins, aggregates e expressions como read-only por padrão | Resultado complexo nunca recebe identidade inventada | META-019 |
| META-021 | P1 | A | Permitir override manual somente em origem inequívoca | Override mostra tabela e colunas exatas | META-019,SEC-014 |
| META-022 | P1 | A | Expor metadata específica por driver sem contaminar o core | Atributos namespaced permanecem acessíveis | META-001 |
| META-023 | P1 | A | Comparar metadata carregada com colunas retornadas | Divergência bloqueia edição e solicita refresh | META-003 |
| META-024 | P0 | A | Testar metadata live em PostgreSQL e MySQL | PK composta, unique, generated e defaults passam em containers | META-002 |

### 8.3 Segurança e erros

- Falha parcial de metadata torna a origem read-only.
- Metadata cacheada nunca habilita mutação se as colunas retornadas divergem.
- Identidade vazia ou contendo coluna ausente bloqueia update/delete.
- Identificador manual exige confirmação do usuário, não inferência silenciosa.

## 9. DATA-EDIT — Edição individual e change sets

### 9.1 Estado de linha

```text
Clean
Inserted
Updated
Deleted
Conflict
ApplyFailed
```

Cada linha editável preserva valores originais, valores atuais, identidade, versão
local e erros por célula. O `ChangeSetService` pertence à aba `TableData` e nunca é
global ao workspace.

### 9.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| DATA-EDIT-001 | P0 | E | Associar `ChangeSet` a cada aba `TableData` | Duas tabelas mantêm mudanças independentes | BASE,META-001 |
| DATA-EDIT-002 | P0 | A | Construir linhas editáveis a partir de page + metadata | Linha contém original, current e identidade | META-003,META-006 |
| DATA-EDIT-003 | P0 | A | Abrir editor inline de célula | Enter/double-click inicia edição somente quando permitido | DATA-EDIT-002 |
| DATA-EDIT-004 | P0 | A | Confirmar ou cancelar edição local | Enter grava localmente; Escape restaura valor anterior | DATA-EDIT-003 |
| DATA-EDIT-005 | P0 | A | Validar valor conforme tipo lógico/nativo | Erro aparece na célula e impede apply | META-016 |
| DATA-EDIT-006 | P0 | A | Diferenciar null, vazio, bytes vazios e default | Preview e driver recebem estados distintos | META-004 |
| DATA-EDIT-007 | P0 | A | Bloquear edição de coluna generated/read-only | UI explica a metadata responsável | META-005 |
| DATA-EDIT-008 | P0 | E | Inserir nova linha local | Linha aparece como inserted sem I/O remoto | META-004 |
| DATA-EDIT-009 | P0 | A | Omitir colunas default/generated no insert | Server defaults funcionam sem enviar null | META-004,META-005 |
| DATA-EDIT-010 | P0 | A | Clonar linha selecionada | Clone vira insert e limpa valores generated | DATA-EDIT-008 |
| DATA-EDIT-011 | P0 | E | Marcar linha para delete local | Linha permanece visível com estado deleted | DATA-EDIT-002 |
| DATA-EDIT-012 | P0 | A | Desfazer delete antes do apply | Linha retorna ao estado anterior | DATA-EDIT-011 |
| DATA-EDIT-013 | P0 | E | Produzir update apenas com colunas alteradas | DML não reenvia colunas intactas no SET | DATA-EDIT-002 |
| DATA-EDIT-014 | P0 | E | Produzir predicate por identidade e original relevante | Conflitos resultam em affected != 1 | META-006 |
| DATA-EDIT-015 | P0 | A | Mostrar marcadores inserted/updated/deleted/conflict | Estado é distinguível com e sem cor | UX-006 |
| DATA-EDIT-016 | P0 | A | Exibir contador permanente de mudanças pendentes | Header/status mostra contagem por aba | DATA-EDIT-001 |
| DATA-EDIT-017 | P0 | A | Undo/redo de operações locais | Edit, insert, clone e delete são reversíveis | DATA-EDIT-003 |
| DATA-EDIT-018 | P0 | E | Reverter célula, linha ou todas as mudanças | Escopo escolhido retorna ao original | DATA-EDIT-001 |
| DATA-EDIT-019 | P0 | E | Abrir revisão antes de apply | Review lista alvo, operações e riscos | DATA-EDIT-001 |
| DATA-EDIT-020 | P0 | E | Mostrar original -> novo por operação | Usuário inspeciona diff sem SQL bruto obrigatório | DATA-EDIT-019 |
| DATA-EDIT-021 | P0 | E | Mostrar DML parametrizado e parâmetros sanitizados | Valores não são concatenados no SQL | DATA-EDIT-019 |
| DATA-EDIT-022 | P0 | A | Aplicar mudanças selecionadas ou todas | Mudanças fora do escopo permanecem pendentes | DATA-EDIT-019 |
| DATA-EDIT-023 | P0 | E | Aplicar atomicamente por padrão | Falha em uma operação reverte o lote | DATA-EDIT-022 |
| DATA-EDIT-024 | P0 | E | Validar uma linha afetada por update/delete | Zero ou múltiplas linhas geram conflito | DATA-EDIT-014 |
| DATA-EDIT-025 | P0 | A | Preservar change set em falha | Usuário corrige e tenta novamente | DATA-EDIT-023 |
| DATA-EDIT-026 | P0 | A | Marcar conflito por linha/operação | Review identifica exatamente o item falho | DATA-EDIT-024 |
| DATA-EDIT-027 | P0 | A | Resolver conflito com Reload Remote | Linha atualiza e mudança local é revista | DATA-EDIT-026 |
| DATA-EDIT-028 | P0 | A | Resolver conflito com Keep Local | Nova tentativa exige novo preview | DATA-EDIT-026 |
| DATA-EDIT-029 | P0 | A | Resolver conflito com Revert Local | Mudança é removida sem afetar outras | DATA-EDIT-026 |
| DATA-EDIT-030 | P1 | A | Permitir force overwrite somente por política explícita | Ação não é padrão e exige confirmação forte | SEC-013,DATA-EDIT-026 |
| DATA-EDIT-031 | P0 | A | Recarregar após sucesso preservando contexto | Filtro, sort, página e seleção estável sobrevivem | DATA-EDIT-023 |
| DATA-EDIT-032 | P0 | A | Bloquear refresh/paginação que descarte pendências | Prompt oferece apply, discard ou cancel | DATA-EDIT-001 |
| DATA-EDIT-033 | P0 | A | Bloquear fechamento/troca de projeto com pendências | Mesma decisão explícita em todos os caminhos | BASE,DATA-EDIT-001 |
| DATA-EDIT-034 | P1 | A | Recuperar mudanças locais após crash | Recovery restaura como não aplicadas | QUALITY-018 |
| DATA-EDIT-035 | P1 | A | Editor expandido de valor reutilizável | Texto, JSON, XML, array, binary e image usam viewer adequado | GRID-030 |
| DATA-EDIT-036 | P1 | A | Completion para enum e FK quando disponível | Sugestões não bloqueiam entrada manual válida | META-013,META-015 |
| DATA-EDIT-037 | P1 | A | Copiar/pastar uma célula preservando tipo | Paste passa por parser/validator do driver | DATA-EDIT-005 |
| DATA-EDIT-038 | P0 | A | Action surface por teclado, mouse e palette | Toda operação é alcançável pelos três caminhos | UX-001 |
| DATA-EDIT-039 | P0 | A | Teste vertical edit/update/delete PostgreSQL | Abertura real -> edit -> review -> apply -> reload passa | META-024 |
| DATA-EDIT-040 | P0 | A | Teste vertical edit/update/delete MySQL | Mesmo fluxo passa com semântica MySQL | META-024 |

## 10. BULK — Operações em lote

### 10.1 Modelo

`BulkMutationPlan` é imutável após confirmação e registra:

- origem e target;
- seleção materializada por identidades;
- transformação tipada;
- quantidade conhecida ou estimada;
- preview representativo;
- política de transação;
- chunk size quando aplicável;
- limites e confirmação;
- estratégia de conflito.

### 10.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| BULK-001 | P0 | P | Selecionar múltiplas linhas contíguas | Range permanece estável durante navegação local | GRID-001 |
| BULK-002 | P0 | P | Selecionar linhas não contíguas | Pick set alimenta copy e mutação | GRID-002 |
| BULK-003 | P0 | A | Materializar seleção em identidades antes do apply | Sort/filter visual não muda o alvo confirmado | META-006 |
| BULK-004 | P0 | A | Marcar todas as linhas selecionadas para delete | Review lista quantidade e identidades | DATA-EDIT-011,BULK-003 |
| BULK-005 | P0 | A | Alterar uma coluna em todas as linhas selecionadas | Um valor validado gera updates individuais seguros | DATA-EDIT-005,BULK-003 |
| BULK-006 | P0 | A | Definir seleção como null | Coluna non-null bloqueia antes do preview | META-004 |
| BULK-007 | P0 | A | Definir seleção como default | Driver gera semântica suportada ou explica indisponibilidade | META-004 |
| BULK-008 | P0 | A | Fill down a partir da célula líder | Preview mostra quantidade alterada | BULK-005 |
| BULK-009 | P1 | A | Find/replace na seleção de texto | Transformação é local, revisável e escapada | BULK-005 |
| BULK-010 | P0 | A | Colar matriz TSV/CSV sobre células | Dimensões e tipos são validados antes da alteração | DATA-EDIT-037 |
| BULK-011 | P0 | A | Colar múltiplas linhas como inserts | Header/mapeamento e defaults são revisáveis | DATA-EDIT-008 |
| BULK-012 | P1 | A | Permitir mapeamento manual de colunas no paste | Colunas ausentes/extra são explicitadas | BULK-011 |
| BULK-013 | P1 | A | Transformações tipadas aprovadas pelo driver | Operação indisponível explica tipo ou driver | DRIVER-006 |
| BULK-014 | P0 | A | Preview com target, count, identity e risco | Usuário conhece alcance antes de confirmar | SEC-006 |
| BULK-015 | P0 | A | Limite configurável de linhas | Operação acima do limite exige override permitido | SEC-007 |
| BULK-016 | P0 | A | Limite mais forte em produção | Confirmação digitada inclui target e count | SEC-008 |
| BULK-017 | P0 | A | Execução atômica por padrão | Qualquer falha reverte todo o lote | DATA-EDIT-023 |
| BULK-018 | P1 | A | Chunks opcionais para lotes grandes | UI declara que commits parciais podem ocorrer | TASK-001 |
| BULK-019 | P1 | A | Progresso por chunk | Rows completed/failed ficam visíveis | BULK-018 |
| BULK-020 | P1 | A | Cancelamento antes do commit atômico | Cancel não deixa alteração parcial | BULK-017 |
| BULK-021 | P1 | A | Cancelamento entre chunks | Relatório identifica commits concluídos | BULK-018 |
| BULK-022 | P0 | A | Conflito preserva itens não aplicados | Usuário revisa retry/revert por item | DATA-EDIT-026 |
| BULK-023 | P1 | A | Exportar relatório de lote | Relatório sanitizado inclui counts e erros | QUERY-020 |
| BULK-024 | P0 | A | Testar bulk delete/update live nos dois drivers | PK simples/composta e conflito passam | DATA-EDIT-039,DATA-EDIT-040 |
| BULK-025 | P0 | A | Testar paste com null/default/generated | Semântica permanece correta nos dois drivers | BULK-010,META-005 |

## 11. GRID — Data grid profissional

### 11.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| GRID-001 | P0 | P | Seleção contígua por teclado e mouse | Range visível alimenta copy/edit | BASE |
| GRID-002 | P0 | P | Seleção não contígua | Toggle de linhas preserva cursor | BASE |
| GRID-003 | P0 | P | Virtualização de linhas | 100k linhas não geram widgets offscreen | BASE |
| GRID-004 | P0 | P | Paginação server-side | Next/prev preservam contexto | BASE |
| GRID-005 | P1 | A | Page size configurável por aba/tabela | Mudança recarrega com aviso de pendências | DATA-EDIT-032 |
| GRID-006 | P1 | A | Fetch all explícito e cancelável | Aviso mostra estimativa e limite de memória | PERF-007 |
| GRID-007 | P1 | A | Total count opcional | Count não bloqueia primeira página | TASK-001 |
| GRID-008 | P0 | P | Sort remoto tipado | Header mostra ordem e prioridade | BASE |
| GRID-009 | P1 | A | Sort local do cache atual | UI distingue local de server | GRID-008 |
| GRID-010 | P0 | P | Filter remoto tipado | Valores permanecem parâmetros | SEC-003 |
| GRID-011 | P1 | A | Filter local do cache atual | Badge local evita falsa expectativa | GRID-010 |
| GRID-012 | P1 | A | Quick filter pelo valor da célula | Eq/not-eq/null geram AST tipada | GRID-010 |
| GRID-013 | P1 | A | Builder AND/OR/NOT | Preview textual descreve expressão | GRID-010 |
| GRID-014 | P1 | A | Histórico de filtros por tabela | Usuário reaplica e limpa condições anteriores | GRID-010 |
| GRID-015 | P1 | A | Busca dentro do resultado carregado | Next/prev match sem query remota | BASE |
| GRID-016 | P1 | A | Auto-refresh configurável | Pausa com pendências e não repete writes | DATA-EDIT-032 |
| GRID-017 | P1 | A | Redimensionar colunas | Mouse/teclado alteram largura persistível | UX-001 |
| GRID-018 | P1 | A | Reordenar colunas | Ordem visual não muda metadata/identity | META-023 |
| GRID-019 | P1 | P | Ocultar colunas | Hide/show tem action surface completa | UX-001 |
| GRID-020 | P1 | P | Congelar colunas | Frozen permanece visível durante pan | UX-001 |
| GRID-021 | P1 | A | Persistir layout por tabela | Width/order/hidden/frozen restauram por projeto | BASE |
| GRID-022 | P1 | P | Copiar célula, linha, coluna e range | Escopo escolhido é exato | BASE |
| GRID-023 | P1 | P | Copiar CSV/TSV/JSON/Markdown/SQL/Text | Null e vazios permanecem distintos | BASE |
| GRID-024 | P1 | A | Extractors configuráveis | Preset por projeto controla formato | FILES-016 |
| GRID-025 | P1 | A | Advanced copy com header/quote/null options | Preview representa saída final | GRID-024 |
| GRID-026 | P1 | A | Advanced paste com detecção de delimitador | Dados passam por mapping e validação | BULK-010 |
| GRID-027 | P1 | A | Exportar célula/seleção/página/resultado | Escopo aparece no preview | TRANSFER-001 |
| GRID-028 | P1 | A | Agregações da seleção | Count/sum/min/max/avg respeitam tipos | GRID-001 |
| GRID-029 | P1 | A | Record view navegável | Uma linha aparece campo a campo | BASE |
| GRID-030 | P1 | P | Viewer especializado de valor | JSON/XML/text/binary/image têm representação segura | BASE |
| GRID-031 | P1 | A | Record view editável | Usa o mesmo validator/change set da grade | DATA-EDIT-003 |
| GRID-032 | P1 | P | Carregar valor grande sob demanda | Prefixo não é confundido com valor completo | GRID-030 |
| GRID-033 | P1 | A | Carregamento incremental de LOB | Viewer mostra loaded/total e cancel | TASK-001 |
| GRID-034 | P1 | A | Navegar FK de saída | Escolha aparece quando houver mais de uma | META-013 |
| GRID-035 | P1 | A | Navegar FK de entrada | Resultado relacionado abre em nova aba | META-014 |
| GRID-036 | P1 | A | Formatação de timezone e locale | Display muda sem alterar valor original | BASE |
| GRID-037 | P1 | A | Formatação decimal configurável | Copy raw e display permanecem distintos | GRID-036 |
| GRID-038 | P1 | A | Encoding explícito para text/binary | Conversão mostra perdas antes de aplicar/exportar | BASE |
| GRID-039 | P0 | A | Mensagens de empty/loading/error/offline | Cada estado preserva ações válidas | UX-008 |
| GRID-040 | P0 | A | Testes snapshot amplo/reduzido/compacto | Seleção, pendências e erros são legíveis | QUALITY-008 |

## 12. NAV — Explorer e busca de objetos

### 12.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| NAV-001 | P0 | P | Árvore lazy por conexão/catalog/schema | Expandir carrega apenas o subtree | BASE |
| NAV-002 | P0 | P | Cache offline com timestamp | Árvore abre sem rede e mostra idade | BASE |
| NAV-003 | P0 | P | Refresh de nó | Somente o nó escolhido é invalidado | BASE |
| NAV-004 | P0 | P | Refresh de subtree | Descendentes são recarregados sem limpar irmãos | BASE |
| NAV-005 | P1 | P | Refresh de schema/database/tudo | Escopo e custo ficam visíveis | TASK-001 |
| NAV-006 | P0 | P | Estados loading/error/stale/restricted | Estado não depende apenas de cor | UX-006 |
| NAV-007 | P1 | P | Filtrar por nome | Resultado atualiza sem bloquear input | PERF-003 |
| NAV-008 | P1 | P | Filtrar por schema e tipo | Configuração persiste no projeto | BASE |
| NAV-009 | P1 | P | Ocultar objetos de sistema | Estado por conexão é persistido | BASE |
| NAV-010 | P1 | P | Mostrar somente favoritos | Favorito preserva identidade original | FILES-012 |
| NAV-011 | P1 | A | Busca global entre conexões | Resultado inclui conexão/schema/tipo | NAV-002 |
| NAV-012 | P1 | A | Buscar por coluna | Coluna abre seu object editor/tabela | NAV-011 |
| NAV-013 | P1 | A | Buscar comentários | Driver indisponível explica limitação | DRIVER-004 |
| NAV-014 | P1 | A | Buscar definições/DDL | Busca online é cancelável e limitada | TASK-001 |
| NAV-015 | P1 | A | Ranking por match, favorito e recência | Ordem é determinística e testada | FILES-012 |
| NAV-016 | P1 | A | Grupos e tags visíveis de conexões | Reordenação não altera escopo do projeto | BASE |
| NAV-017 | P1 | A | Multi-select de objetos homogêneos | Actions mostram interseção de capacidades | DRIVER-001 |
| NAV-018 | P1 | A | Multi-select heterogêneo seguro | Actions incompatíveis são desabilitadas com razão | DRIVER-001 |
| NAV-019 | P1 | P | Abrir Properties/Data/DDL | Objeto abre em aba tipada correta | BASE |
| NAV-020 | P1 | P | Dependências e dependentes | Navegação preserva breadcrumb | OBJECT-020 |
| NAV-021 | P1 | A | Find usages sobre cache e SQL conhecido | Resultados indicam grau de confiança | QUERY-013 |
| NAV-022 | P1 | A | Breadcrumb navegável | Cada segmento abre ou seleciona seu objeto | BASE |
| NAV-023 | P1 | A | Action sheet contextual | Teclado/mouse/palette usam mesmas actions | UX-001 |
| NAV-024 | P1 | A | Copiar nome simples/qualificado/ID/DDL | Opções respeitam quoting do driver | OBJECT-017 |
| NAV-025 | P1 | A | Open in new tab | Não sobrescreve aba do objeto já aberta | BASE |
| NAV-026 | P1 | A | Reveal object from SQL | Resolução seleciona e expande o caminho | NAV-001 |
| NAV-027 | P1 | A | Reveal object from result | Source conhecida seleciona o objeto correto | META-019 |
| NAV-028 | P0 | A | Isolar erro por nó | Falha não limpa a árvore nem perde seleção | QUALITY-006 |
| NAV-029 | P0 | A | Testar árvore grande | 100k objetos cacheados mantêm input responsivo | PERF-003 |
| NAV-030 | P0 | A | Testar paridade mouse/keyboard/palette | Ações produzem effects equivalentes | UX-001 |

## 13. OBJECT — Object editor e operações de schema

### 13.1 Composição

O object editor monta tabs internas conforme o objeto e o driver:

```text
Properties | Data | DDL | Columns | Keys | Indexes | Triggers
Dependencies | Privileges | Statistics | Diagram
```

### 13.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| OBJECT-001 | P1 | P | Object editor por identidade estável | Reabrir objeto foca a mesma aba | BASE |
| OBJECT-002 | P1 | A | Compor seções por capacidade | Seção ausente explica limitação | DRIVER-001 |
| OBJECT-003 | P1 | P | Properties genéricas e específicas | Atributos namespaced permanecem legíveis | META-022 |
| OBJECT-004 | P1 | P | Data para table/view | Usa `TableData`, não grid duplicado | GRID-001 |
| OBJECT-005 | P1 | P | DDL carregado e copiável | Source e runnable DDL são distinguidos | DRIVER-005 |
| OBJECT-006 | P1 | A | Columns com detalhes completos | Tipo/default/generated/key ficam visíveis | META-003 |
| OBJECT-007 | P1 | A | Keys e constraints | PK/unique/FK/check têm propriedades e navegação | META-006 |
| OBJECT-008 | P1 | A | Indexes | Método, colunas, expressão e uniqueness aparecem | DRIVER-004 |
| OBJECT-009 | P1 | A | Triggers | Timing/event/function/body conforme driver | DRIVER-004 |
| OBJECT-010 | P1 | P | Dependencies/dependents | Objetos relacionados são navegáveis | NAV-020 |
| OBJECT-011 | P1 | P | Privileges efetivos e grants | Restrição de visibilidade é declarada | SEC-020 |
| OBJECT-012 | P1 | A | Statistics | Informação disponível é paginada e atualizável | DRIVER-010 |
| OBJECT-013 | P1 | A | Create table por formulário tipado | Preview DDL é obrigatório | OBJECT-030 |
| OBJECT-014 | P1 | A | Create view/routine/index | Campos específicos vêm do driver | DRIVER-005 |
| OBJECT-015 | P1 | A | Duplicate object | Novo nome e dependências são revisados | OBJECT-030 |
| OBJECT-016 | P1 | A | Rename object | Dependências e risco são mostrados | OBJECT-030 |
| OBJECT-017 | P1 | P | SQL generator DDL | Output usa renderer do driver | DRIVER-005 |
| OBJECT-018 | P1 | A | SQL generator SELECT | Colunas explícitas e nome qualificado | META-003 |
| OBJECT-019 | P1 | A | SQL generator INSERT/UPDATE/DELETE | Saída parametrizada usa identity quando necessária | META-006 |
| OBJECT-020 | P1 | A | SQL generator por dependência | Ordem de create/drop é determinística | BASE |
| OBJECT-021 | P1 | A | Abrir SQL gerado em nova aba | Nada é executado automaticamente | BASE |
| OBJECT-022 | P1 | A | Add/alter/drop column | Tipo/default/nullability têm preview e risco | OBJECT-030 |
| OBJECT-023 | P1 | A | Add/drop key e FK | Lock e validação potencial ficam visíveis | OBJECT-030 |
| OBJECT-024 | P1 | A | Add/alter/drop index | Online/concurrent aparece quando suportado | DRIVER-005 |
| OBJECT-025 | P1 | A | Create/alter/drop trigger | Driver declara capacidade real | DRIVER-005 |
| OBJECT-026 | P1 | A | Alter view/routine source | Conflito externo bloqueia apply silencioso | QUALITY-014 |
| OBJECT-027 | P1 | A | Drop object | Confirmação lista dependentes e irreversibilidade | SEC-009 |
| OBJECT-028 | P1 | A | Truncate table | Confirmação forte e comportamento identity explícito | SEC-009 |
| OBJECT-029 | P1 | A | Multi-object operations | Preview lista ordem e falha parcial possível | NAV-017 |
| OBJECT-030 | P0 | P | Planner DDL tipado | Plano contém SQL, risco, transactionality e implicit commit | DRIVER-005 |
| OBJECT-031 | P0 | P | Preview recém-gerado obrigatório | Alterar formulário invalida confirmação anterior | SEC-005 |
| OBJECT-032 | P0 | P | Apply protegido | Target/ambiente/transação ficam visíveis | SEC-006 |
| OBJECT-033 | P0 | A | Refresh automático após sucesso | Apenas objetos afetados são invalidados | NAV-003 |
| OBJECT-034 | P0 | A | Preservar formulário após erro | Correção não exige recomeçar | QUALITY-006 |
| OBJECT-035 | P1 | A | Undo local do formulário | Alterações não aplicadas são reversíveis | OBJECT-013 |
| OBJECT-036 | P1 | A | Detectar drift entre preview e apply | Metadata alterada exige novo preview | META-018 |
| OBJECT-037 | P1 | A | Salvar DDL em arquivo | Encoding e overwrite seguem file policy | FILES-006 |
| OBJECT-038 | P0 | A | Testes live de DDL nos dois drivers | Create/alter/drop e rollback/implicit commit passam | QUALITY-021 |
| OBJECT-039 | P1 | A | Cobrir usuários e roles | Properties/privileges/actions específicas aparecem | SEC-020 |
| OBJECT-040 | P1 | A | Extensão por objeto driver-specific | Nova seção não altera enum central a cada tipo | DRIVER-004 |

## 14. FILES — Arquivos, snippets, favoritos e produtividade

### 14.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| FILES-001 | P1 | P | Abrir/salvar arquivos SQL comuns | Arquivo externo permanece fora do storage interno | BASE |
| FILES-002 | P1 | A | Associar diretório ao projeto | Diretório é opcional e explicitamente escolhido | BASE |
| FILES-003 | P1 | A | File tree do projeto | Descobre SQL/text sem copiar arquivos | FILES-002 |
| FILES-004 | P1 | A | Criar arquivo/diretório | Falhas deixam árvore consistente | FILES-003 |
| FILES-005 | P1 | A | Renomear/mover/duplicar | Abas abertas atualizam path atomicamente | FILES-003 |
| FILES-006 | P1 | A | Excluir com confirmação recuperável quando possível | Target exato e dirty state são validados | FILES-003 |
| FILES-007 | P1 | A | Busca textual no projeto | Streaming/cancelamento evitam travar TUI | TASK-001 |
| FILES-008 | P1 | P | Detectar alteração externa | Reload/keep/diff são oferecidos | BASE |
| FILES-009 | P1 | P | Scratch SQL recuperável | Scratch não exige path | BASE |
| FILES-010 | P1 | A | Scratch texto não SQL | Tipo de aba não assume parser SQL | BASE |
| FILES-011 | P1 | P | Snippets por projeto | CRUD e placeholders são alcançáveis | BASE |
| FILES-012 | P1 | P | Favoritos de objetos | Favorito usa identidade estável | BASE |
| FILES-013 | P1 | A | Favoritos de consultas | Nome, conexão opcional e texto são persistidos | QUERY-012 |
| FILES-014 | P1 | A | Bookmarks em arquivos | Linha/coluna se ajustam quando possível | FILES-003 |
| FILES-015 | P1 | A | Histórico de filtros por tabela | Retenção e limpeza são configuráveis | GRID-014 |
| FILES-016 | P1 | A | Presets de extractor/import/export | Preset é versionado por projeto | TRANSFER-001 |
| FILES-017 | P1 | A | Preferências de display por projeto/conexão | Timezone/locale/decimal têm herança clara | GRID-036 |
| FILES-018 | P1 | A | Vincular arquivo a conexão/schema offline | Associação não conecta até execução | BASE |
| FILES-019 | P1 | A | Templates de query | Template cria documento, não executa | QUERY-001 |
| FILES-020 | P1 | A | Quick open de arquivos/recentes | Busca global inclui path e projeto | NAV-011 |
| FILES-021 | P1 | A | Exportar configuração do projeto sem segredos | Import roundtrip exige reentrada de credenciais | SEC-001 |
| FILES-022 | P1 | A | Duplicar projeto com opções explícitas | Arquivos externos não são copiados silenciosamente | BASE |
| FILES-023 | P1 | A | Recovery de arquivos dirty | Conteúdo volta como local não salvo | QUALITY-018 |
| FILES-024 | P1 | A | Política de encoding/newline | Detecção e conversão mostram perdas | GRID-038 |
| FILES-025 | P0 | A | Testes cross-platform de paths | Linux/macOS/Windows passam sem paths hardcoded | QUALITY-022 |

## 15. QUERY — Execução, histórico e query manager

### 15.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| QUERY-001 | P1 | P | Criar console SQL explicitamente | Nenhum fluxo de data cria console sozinho | BASE |
| QUERY-002 | P1 | P | Associar documento a conexão/schema | Header mostra contexto e estado | BASE |
| QUERY-003 | P1 | P | Executar statement/selection/document/script | Result sets e counts ficam correlacionados | BASE |
| QUERY-004 | P1 | P | Cancelar query | Cancel chega ao driver e atualiza status | TASK-001 |
| QUERY-005 | P1 | P | Transação manual e autocommit | Estado permanece visível por sessão | SEC-006 |
| QUERY-006 | P1 | P | Savepoints | Create/rollback/release têm actions e erros claros | QUERY-005 |
| QUERY-007 | P1 | P | Parâmetros tipados | Valores não entram no histórico por padrão | SEC-002 |
| QUERY-008 | P1 | P | Completion contextual | Catálogo cacheado funciona offline | NAV-002 |
| QUERY-009 | P1 | P | Format e diagnostics locais | Dialeto e servidor continuam autoridade final | DRIVER-003 |
| QUERY-010 | P1 | P | Go to definition | Abre/revela objeto quando resolvível | NAV-026 |
| QUERY-011 | P1 | A | Find references em documentos | Resultados distinguem parse confirmado e match textual | NAV-021 |
| QUERY-012 | P1 | P | Histórico pesquisável de SQL do usuário | Timestamp/conexão/status/duração ficam disponíveis | BASE |
| QUERY-013 | P1 | A | Query manager unificado | Usuário, metadata, DDL, task, MCP e interno são categorizados | TASK-001 |
| QUERY-014 | P1 | A | Registrar projeto/conexão/sessão/schema | Evento é correlacionável sem segredo | SEC-002 |
| QUERY-015 | P1 | A | Registrar duração, status, rows e erro sanitizado | Falhas podem ser diagnosticadas localmente | SEC-002 |
| QUERY-016 | P1 | A | Filtrar por período/origem/conexão/status | Consulta do log permanece responsiva | PERF-012 |
| QUERY-017 | P1 | A | Retenção e cleanup configuráveis | Usuário controla volume e privacidade | SEC-002 |
| QUERY-018 | P1 | A | Reabrir query do histórico | Contexto é sugerido, não reconectado/rodado automaticamente | QUERY-012 |
| QUERY-019 | P1 | A | Distinguir query interna de query do usuário | Histórico principal não fica poluído | QUERY-013 |
| QUERY-020 | P1 | A | Registrar operações não SQL por categoria | Bulk/transfer/admin produzem evento correlacionado | QUERY-013 |
| QUERY-021 | P1 | A | Mostrar múltiplos result sets em abas independentes | Cada result set preserva status/notices | QUERY-003 |
| QUERY-022 | P1 | P | Mostrar notices/warnings/server messages | Mensagem pertence à operação correta | QUERY-003 |
| QUERY-023 | P1 | A | Background execution sem bloquear edição | Usuário continua editando durante execução | TASK-001 |
| QUERY-024 | P1 | A | Timeout por operação/preset | Timeout é visível e cancelável | TASK-001 |
| QUERY-025 | P1 | A | Run configuration salva | Configuração referencia arquivo, conexão e parâmetros não secretos | TASK-011 |
| QUERY-026 | P1 | A | Compare resultados de execuções | Result tabs podem alimentar Data Compare | COMPARE-011 |
| QUERY-027 | P1 | A | Explain a partir do statement selecionado | Estimated não exige confirmação destrutiva | QUERY-001 |
| QUERY-028 | P1 | P | Explain Analyze protegido | Confirmação descreve execução real | SEC-009 |
| QUERY-029 | P0 | A | Testar correlação concorrente | Batches/notices/cancel nunca atingem outra aba | QUALITY-010 |
| QUERY-030 | P0 | A | Testar privacidade do histórico | Parâmetros e segredos não aparecem em storage/log | SEC-002 |

## 16. TRANSFER — Import, export, backup e migração de dados

### 16.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| TRANSFER-001 | P1 | P | Wizard/preset de export | Source, scope, format e target são revisados | BASE |
| TRANSFER-002 | P1 | A | Exportar célula/seleção/página | Output contém exatamente o escopo escolhido | GRID-027 |
| TRANSFER-003 | P1 | P | Exportar query/tabela completa | Streaming não depende da página visível | TASK-001 |
| TRANSFER-004 | P1 | A | Exportar múltiplas tabelas/schema | Estrutura de diretório e nomes evitam colisão | NAV-017 |
| TRANSFER-005 | P1 | P | CSV/TSV | Delimitador, quote, escape, header e null são configuráveis | FILES-016 |
| TRANSFER-006 | P1 | P | JSON/JSONL | Tipos e null permanecem representáveis | FILES-016 |
| TRANSFER-007 | P1 | P | SQL inserts | Quoting/literals são específicos por driver | DRIVER-005 |
| TRANSFER-008 | P1 | P | Markdown/Text | Encoding e newline são explícitos | FILES-024 |
| TRANSFER-009 | P1 | P | Encoding configurável | Preview mostra caracteres inválidos/perdas | FILES-024 |
| TRANSFER-010 | P1 | P | Import CSV/TSV/JSON/JSONL | Parser é streaming e cancelável | TASK-001 |
| TRANSFER-011 | P1 | A | Preview de amostra antes do import | Mapping/tipos/erros aparecem antes de escrever | TRANSFER-010 |
| TRANSFER-012 | P1 | A | Auto-map por nome | Ambiguidade exige escolha manual | META-003 |
| TRANSFER-013 | P1 | A | Mapping manual de colunas | Preset pode ser salvo e reaplicado | FILES-016 |
| TRANSFER-014 | P1 | A | Conversão tipada e política de erro | Stop/skip/collect são explícitos | META-016 |
| TRANSFER-015 | P1 | P | Insert em batches | Progress e rows/sec são visíveis | TASK-001 |
| TRANSFER-016 | P1 | A | Update por identidade | Mapping de key é obrigatório | META-006 |
| TRANSFER-017 | P1 | A | Upsert quando suportado | Driver declara semântica e conflitos | DRIVER-007 |
| TRANSFER-018 | P1 | A | Transferir tabela -> tabela na mesma conexão | Nenhum arquivo intermediário é obrigatório | TASK-001 |
| TRANSFER-019 | P1 | A | Transferir entre conexões | Source/destination sessions são independentes | BASE |
| TRANSFER-020 | P1 | A | Transferir PostgreSQL <-> MySQL | Conversões incompatíveis aparecem no preview | DRIVER-008 |
| TRANSFER-021 | P1 | A | Criar tabela destino opcionalmente | DDL passa por review separado | OBJECT-030 |
| TRANSFER-022 | P1 | A | Truncate destino opcional | Confirmação forte é separada do import | OBJECT-028 |
| TRANSFER-023 | P1 | A | Atomicidade configurável | Limites do driver e commits parciais são explicados | TASK-001 |
| TRANSFER-024 | P1 | P | Progresso rows/bytes/rate/ETA | Updates são throttled e não travam render | PERF-011 |
| TRANSFER-025 | P1 | P | Cancelamento | Resultado distingue rollback e parcial | TASK-001 |
| TRANSFER-026 | P1 | A | Retry seguro | Nunca duplica rows sem estratégia idempotente | SEC-004 |
| TRANSFER-027 | P1 | A | Relatório de rejeitados | Arquivo/result tab contém linha, coluna e erro sanitizado | QUERY-020 |
| TRANSFER-028 | P1 | P | Backup nativo PostgreSQL/MySQL | Tool detection e target preview funcionam | DRIVER-009 |
| TRANSFER-029 | P1 | P | Restore nativo protegido | Nunca sobrescreve source path e exige target explícito | SEC-009 |
| TRANSFER-030 | P1 | A | Presets por projeto | Configurações não incluem segredos | FILES-016 |
| TRANSFER-031 | P2 | A | Salvar transfer como task | Task reproduz config sem prompt secreto persistido | TASK-011 |
| TRANSFER-032 | P0 | A | Testes live de roundtrip | Tipos suportados preservam valor nos dois drivers | QUALITY-021 |
| TRANSFER-033 | P0 | A | Testes de cancelamento/cleanup | Temp files/spools são removidos | PERF-015 |
| TRANSFER-034 | P1 | A | Checksum/row count opcional | Relatório detecta divergência pós-transfer | COMPARE-001 |
| TRANSFER-035 | P1 | A | Limites de bytes/rows/duração | Policy interrompe com resultado claro | SEC-007 |

## 17. COMPARE — Schema compare e data compare

### 17.1 Schema compare

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| COMPARE-001 | P1 | P | Snapshot versionado de schema | Live/saved/file são comparáveis | BASE |
| COMPARE-002 | P1 | P | Comparar schemas/databases | Added/removed/changed são classificados | COMPARE-001 |
| COMPARE-003 | P1 | A | Selecionar tipos de objeto | Filtro afeta diff e script | OBJECT-002 |
| COMPARE-004 | P1 | A | Ignorar propriedades configuráveis | Regra salva é visível no relatório | FILES-016 |
| COMPARE-005 | P1 | A | Mapear schemas/namespaces | Source/target mapping é explícito | DRIVER-008 |
| COMPARE-006 | P1 | P | Exibir diff detalhado | Propriedade original/nova fica legível | OBJECT-003 |
| COMPARE-007 | P1 | P | Classificar risco e reversibilidade | Cada mudança herda risco DDL | OBJECT-030 |
| COMPARE-008 | P1 | P | Ordenar por dependência | Script é determinístico e ciclos são relatados | OBJECT-020 |
| COMPARE-009 | P1 | A | Selecionar mudanças para sync | Dependências requeridas são sugeridas | COMPARE-008 |
| COMPARE-010 | P1 | P | Gerar script sem aplicar | Script pode ser salvo/copied/opened | FILES-001 |
| COMPARE-011 | P1 | P | Aplicar plano recém-revisado | Drift invalida confirmação anterior | OBJECT-036 |
| COMPARE-012 | P1 | A | Salvar compare como task | Sources e filtros são persistidos | TASK-011 |

### 17.2 Data compare

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| COMPARE-013 | P1 | A | Comparar duas tabelas | Wizard define source e target | META-003 |
| COMPARE-014 | P1 | A | Comparar query results | Reexecutar ou usar snapshot é escolha explícita | QUERY-026 |
| COMPARE-015 | P1 | A | Comparar múltiplos pares | Cada par possui mapping e status | TASK-001 |
| COMPARE-016 | P1 | A | Auto-map de colunas | Ambiguidade não é resolvida silenciosamente | META-003 |
| COMPARE-017 | P1 | A | Mapping manual | Conversões e perdas são previstas | DRIVER-008 |
| COMPARE-018 | P1 | A | Escolher/inferir key de comparação | Key segue regra de row identity | META-006 |
| COMPARE-019 | P1 | A | Streaming merge/hash compare | Dataset grande respeita memória limitada | COMPARE-014 |
| COMPARE-020 | P1 | A | Exibir only-source/only-target/changed/equal | Counts e exemplos são navegáveis | GRID-003 |
| COMPARE-021 | P1 | A | Diff por coluna | Original/target ficam lado a lado | GRID-029 |
| COMPARE-022 | P1 | A | Gerar sync insert/update/delete | Direção é explícita e reversível apenas quando possível | DATA-EDIT-021 |
| COMPARE-023 | P1 | A | Aplicar sync protegido | Usa preview, limits e transaction policy | BULK-014 |
| COMPARE-024 | P1 | A | Exportar relatório sem aplicar | Formato text/JSON/CSV é selecionável | TRANSFER-001 |
| COMPARE-025 | P1 | A | Comparar PostgreSQL <-> MySQL | Tipo incompatível é diferença, não conversão silenciosa | DRIVER-008 |
| COMPARE-026 | P1 | A | Retomar/cancelar compare | Estado final informa completude | TASK-001 |
| COMPARE-027 | P0 | A | Testar datasets grandes e duplicated keys | Resultado é determinístico e memória limitada | PERF-013 |
| COMPARE-028 | P0 | A | Testar sync com conflito | Nenhuma divergência é aplicada silenciosamente | SEC-004 |

## 18. DIAGRAM — Relações e ERD terminal-first

### 18.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| DIAGRAM-001 | P2 | A | Construir grafo de tabelas/FKs | Nodes/edges refletem metadata atual | META-013,META-014 |
| DIAGRAM-002 | P2 | A | Abrir vizinhança de uma tabela | Profundidade é configurável e limitada | DIAGRAM-001 |
| DIAGRAM-003 | P2 | A | Abrir schema completo | Large graph usa filtros e lazy expansion | DIAGRAM-001 |
| DIAGRAM-004 | P2 | A | Navegação por teclado | Next node/edge e open object são alcançáveis | UX-001 |
| DIAGRAM-005 | P2 | A | Pan/zoom semântico textual | Usuário muda escopo, não pixels | UX-003 |
| DIAGRAM-006 | P2 | A | Buscar e filtrar nodes | Schema/name/type/favorite são suportados | NAV-011 |
| DIAGRAM-007 | P2 | A | Encontrar caminho entre tabelas | Caminho é exibido e exportável | DIAGRAM-001 |
| DIAGRAM-008 | P2 | A | Detectar ciclos/dependências | Resultado aponta objetos envolvidos | DIAGRAM-001 |
| DIAGRAM-009 | P2 | A | Layout textual compacto | Grafo pequeno é legível no terminal | UX-007 |
| DIAGRAM-010 | P2 | A | Exportar Mermaid | Output passa em parser/fixture | FILES-001 |
| DIAGRAM-011 | P2 | A | Exportar Graphviz DOT | Identifiers são escapados corretamente | FILES-001 |
| DIAGRAM-012 | P2 | A | Exportar representação textual | Funciona sem Unicode/cor | UX-006 |
| DIAGRAM-013 | P2 | A | Diagramas personalizados por projeto | Seleção de nodes e notas simples persistem | BASE |
| DIAGRAM-014 | P2 | A | Gerar SQL para seleção | Reusa `SqlGenerator` | OBJECT-017 |
| DIAGRAM-015 | P3 | F | Editar schema visualmente | Só após object planner e undo maduros | OBJECT-038 |
| DIAGRAM-016 | P0 | A | Testes determinísticos de layout/export | Mesmo grafo gera output estável | QUALITY-012 |

## 19. TASK — Background jobs, tarefas salvas e automação

### 19.1 Estado

```text
Queued -> Running -> Paused/Retrying -> Completed/Failed/Cancelled
```

### 19.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| TASK-001 | P0 | P | Contrato comum de background operation | Query/transfer/compare/admin compartilham lifecycle | BASE |
| TASK-002 | P0 | P | Operation ID e correlação | Updates nunca atingem outra aba/task | TASK-001 |
| TASK-003 | P0 | P | Progresso tipado | Rows/bytes/stage/message são opcionais e coerentes | TASK-001 |
| TASK-004 | P0 | P | Cancelamento cooperativo | Runtime e driver recebem token | TASK-001 |
| TASK-005 | P1 | A | Background task center | Lista status, origem, duração e ações | TASK-001 |
| TASK-006 | P1 | A | Logs sanitizados por task | Usuário exporta sem segredos | SEC-002 |
| TASK-007 | P1 | A | Retry somente quando seguro | Política declara idempotência/restartability | TASK-001 |
| TASK-008 | P1 | A | Pause/resume quando suportado | Capability indisponível explica razão | DRIVER-001 |
| TASK-009 | P1 | A | Result artifact | Arquivo/report/tab fica associado à task | FILES-001 |
| TASK-010 | P1 | A | Cleanup de tasks antigas | Retenção é configurável | QUERY-017 |
| TASK-011 | P2 | A | Persistir task definition | Config versionada não contém segredos | SEC-001 |
| TASK-012 | P2 | A | SQL script task | Arquivo/conexão/params não secretos são definidos | QUERY-025 |
| TASK-013 | P2 | A | Export/import task | Reusa preset de transfer | TRANSFER-031 |
| TASK-014 | P2 | A | Backup/restore task | Secret é solicitado em runtime | TRANSFER-028 |
| TASK-015 | P2 | A | Schema/data compare task | Reusa compare config | COMPARE-012 |
| TASK-016 | P2 | A | Maintenance task | Ação e target são driver-specific | ADMIN-020 |
| TASK-017 | P2 | A | Composite task sequencial | Output pode alimentar próximo passo explicitamente | TASK-011 |
| TASK-018 | P2 | A | Política stop/continue on error | Resultado final enumera cada etapa | TASK-017 |
| TASK-019 | P2 | A | Variáveis não secretas | Resolução tem preview antes de executar | TASK-011 |
| TASK-020 | P2 | A | Secret placeholders | Valor vem do keychain/prompt e não persiste | SEC-001 |
| TASK-021 | P2 | A | Exportar comando/config para scheduler | cron/systemd/Task Scheduler recebem artefato explícito | TASK-011 |
| TASK-022 | P2 | A | Não exigir daemon Dexo | Agendamento externo chama CLI não interativa | TASK-021 |
| TASK-023 | P2 | A | CLI list/run/status/cancel | Automação possui output estruturado | TASK-005 |
| TASK-024 | P2 | A | MCP read status/cancel governado | Grants limitam visibilidade e ação | TASK-001 |
| TASK-025 | P0 | A | Recovery após crash | Task mutável fica failed/unknown, nunca auto-reexecuta | SEC-004 |
| TASK-026 | P0 | A | Testar cancel/retry/correlation sob concorrência | Stress não mistura eventos | QUALITY-010 |
| TASK-027 | P1 | A | Notificação de conclusão | TUI mostra conclusão sem roubar foco | UX-012 |
| TASK-028 | P1 | A | Limitar concorrência por conexão/global | Queue evita exaurir banco e máquina | TASK-002 |

## 20. ADMIN — Sessões, locks, manutenção e monitoramento

### 20.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| ADMIN-001 | P1 | P | Listar sessões | User/db/state/duration/query sanitizada aparecem | DRIVER-010 |
| ADMIN-002 | P1 | P | Listar locks | Lock mode/granted/target são exibidos | DRIVER-010 |
| ADMIN-003 | P1 | P | Blocking graph | Blocker/waiter são navegáveis | ADMIN-002 |
| ADMIN-004 | P1 | A | Atualização periódica configurável | Pause/resume e interval ficam visíveis | TASK-001 |
| ADMIN-005 | P1 | A | Cancelar query | Target e privilégio são confirmados | SEC-009 |
| ADMIN-006 | P1 | A | Terminar sessão | Confirmação forte distingue cancel de terminate | SEC-009 |
| ADMIN-007 | P1 | P | Mostrar tamanhos | Pagination/sort e unidade são consistentes | GRID-004 |
| ADMIN-008 | P1 | P | Mostrar variáveis/settings | Source e escopo do valor aparecem | DRIVER-010 |
| ADMIN-009 | P1 | P | Mostrar statistics | Timestamp e necessidade de refresh aparecem | DRIVER-010 |
| ADMIN-010 | P1 | A | Statistics por objeto no object editor | Link abre contexto correspondente | OBJECT-012 |
| ADMIN-011 | P1 | A | Slow queries quando suportado | Fonte/extensão necessária é explicada | DRIVER-010 |
| ADMIN-012 | P1 | A | Filtros de sessão/query/lock | Filtro é local e não mutável | GRID-011 |
| ADMIN-013 | P1 | A | Exportar snapshot de admin | Output é sanitizado | SEC-002 |
| ADMIN-014 | P1 | A | Comparar snapshots | Diferenças de count/state são resumidas | COMPARE-020 |
| ADMIN-015 | P1 | P | Explain estimated | Plano tree/table/summary é preservado | DRIVER-011 |
| ADMIN-016 | P1 | P | Explain save/compare | Plan artifact é versionado | FILES-001 |
| ADMIN-017 | P1 | P | Explain a partir do SQL ativo | Statement correto é selecionado | QUERY-027 |
| ADMIN-018 | P1 | P | Explain Analyze protegido | Confirma execução e risco de write/locks | QUERY-028 |
| ADMIN-019 | P1 | P | Preview de maintenance action | SQL/impact/privilégio são mostrados | DRIVER-010 |
| ADMIN-020 | P1 | P | Vacuum/analyze/reindex/optimize equivalentes | Driver oferece apenas ações válidas | DRIVER-010 |
| ADMIN-021 | P1 | A | Progresso/log de maintenance | Long running action vira task | TASK-001 |
| ADMIN-022 | P1 | A | Capability/privilégio por ação | Ação indisponível tem razão concreta | SEC-020 |
| ADMIN-023 | P1 | A | Produção exige política reforçada | Target e categoria administrativa são digitados | SEC-008 |
| ADMIN-024 | P1 | A | Audit local de ações admin | Actor/origin/target/outcome são registrados | SEC-015 |
| ADMIN-025 | P0 | A | Testes live por versão suportada | Sessões/locks/actions válidas passam na matriz | DRIVER-012 |
| ADMIN-026 | P1 | A | Falha de permissão não derruba painel | Seções restantes continuam utilizáveis | QUALITY-006 |
| ADMIN-027 | P2 | A | Dashboard terminal compacto | Sessões/locks/size/throughput sem polling excessivo | PERF-011 |
| ADMIN-028 | P2 | A | Alertas locais configuráveis | Threshold gera notificação, não daemon obrigatório | TASK-027 |

## 21. SEC — Segurança, políticas, privilégios e auditoria

### 21.1 Classificação

Operações são classificadas como:

```text
Read
Write
Destructive
LockSensitive
Administrative
SecretHandling
```

### 21.2 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| SEC-001 | P0 | P | Segredos somente no keychain/prompt de sessão | SQLite/export/log não contém credencial | BASE |
| SEC-002 | P0 | P | Sanitização central de logs/history/diagnostics | Fixtures com secrets não aparecem no output | SEC-001 |
| SEC-003 | P0 | P | Valores mutáveis sempre parametrizados | SQL concatenado com valor do usuário é rejeitado em review | DRIVER-005 |
| SEC-004 | P0 | P | Não repetir mutações automaticamente | Retry exige operação idempotente ou novo review | TASK-007 |
| SEC-005 | P0 | P | Preview vinculado à revisão atual | Alterar target/input invalida confirmação | BASE |
| SEC-006 | P0 | P | Target/session/environment sempre visíveis | Review e status identificam destino exato | BASE |
| SEC-007 | P0 | A | Limites centrais de rows/bytes/time | Adapter não consegue contornar policy | BASE |
| SEC-008 | P0 | P | Política reforçada de produção | Confirmação digitada contém target/count/categoria | SEC-006 |
| SEC-009 | P0 | P | Confirmação forte para destructive/admin | Ação não usa confirmação genérica ambígua | SEC-006 |
| SEC-010 | P0 | P | Read-only aplicado em app service | TUI/CLI/MCP recebem o mesmo bloqueio | BASE |
| SEC-011 | P0 | A | Read-only por projeto/conexão/sessão | Escopo efetivo e fonte da política são visíveis | SEC-010 |
| SEC-012 | P0 | A | Policy decision estruturada | Allowed/denied/confirm inclui razão e requirements | SEC-010 |
| SEC-013 | P0 | A | Force operation fora do caminho padrão | Command exige policy e confirmação específica | SEC-012 |
| SEC-014 | P0 | A | Override de editabilidade governado | Escolha manual nunca elimina affected-row check | META-010 |
| SEC-015 | P1 | P | Audit local de ações mutáveis | Timestamp/origin/target/category/outcome ficam registrados | SEC-002 |
| SEC-016 | P1 | A | Correlation ID no audit | Evento liga preview, task e resultado | TASK-002 |
| SEC-017 | P1 | A | Retenção/cleanup de audit configuráveis | Cleanup exige ação explícita ou policy local | SEC-015 |
| SEC-018 | P1 | A | Export de audit sanitizado | Secret scan passa no artefato | SEC-002 |
| SEC-019 | P1 | P | Grants e roles visíveis | Informação parcial declara restrições | OBJECT-011 |
| SEC-020 | P1 | A | Capability por privilégio efetivo | Botão indisponível explica privilégio necessário | DRIVER-010 |
| SEC-021 | P1 | A | Alterar grants com preview | GRANT/REVOKE usa planner e audit | OBJECT-030 |
| SEC-022 | P1 | A | Gerenciar users/roles/passwords | Password nunca aparece em argv/log/preview | SEC-001 |
| SEC-023 | P0 | P | TLS verificado por padrão | Opção insegura exige escolha e aviso persistente | BASE |
| SEC-024 | P0 | P | SSH known-hosts estrito | Chave nova/alterada exige confirmação | BASE |
| SEC-025 | P0 | A | Secret redaction property/fuzz tests | Variações de credenciais não vazam | QUALITY-004 |
| SEC-026 | P0 | A | Threat tests de todos os adapters | TUI/CLI/MCP respeitam policies idênticas | QUALITY-019 |
| SEC-027 | P1 | A | Clipboard warning para dados sensíveis configurados | Policy pode bloquear/cancelar copy massivo | GRID-025 |
| SEC-028 | P1 | A | Arquivos temporários com permissões restritas | Spools/dumps não ficam world-readable | BASE |
| SEC-029 | P0 | A | Crash recovery nunca reaplica mutação | Recovery restaura somente estado local | DATA-EDIT-034 |
| SEC-030 | P0 | A | Diagnostics opt-in e previewável | Usuário vê conteúdo antes de gravar | SEC-002 |

## 22. MCP — Exposição governada das novas capacidades

### 22.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| MCP-001 | P0 | P | MCP continua stdio e server-only | Nenhum listener HTTP é criado | BASE |
| MCP-002 | P0 | P | Profiles começam disabled/read-only | Bootstrap não expõe writes | SEC-010 |
| MCP-003 | P0 | P | MCP não cria o próprio grant | Tentativa protocolar é rejeitada | SEC-012 |
| MCP-004 | P0 | P | Grant temporário e escopado | Expiração/revogação removem tools mutáveis | SEC-012 |
| MCP-005 | P1 | A | Cada tool declara capability e limites | Schema/documentação mostram scope/rows/bytes/time | DRIVER-001 |
| MCP-006 | P1 | A | Metadata/search read-only | Allowlist restringe conexões e objetos | NAV-011 |
| MCP-007 | P1 | A | Leitura de data paginada | Limits e cancellation são aplicados | GRID-004 |
| MCP-008 | P1 | E | Mutação usa app service e policy central | Tool não constrói DML paralelo | DATA-EDIT-023 |
| MCP-009 | P1 | A | DDL/compare sync exigem grants específicos | Read grant nunca permite schema/data writes | COMPARE-023 |
| MCP-010 | P1 | A | Task status/cancel com escopo | Cliente só vê tasks autorizadas | TASK-024 |
| MCP-011 | P1 | A | Admin read e admin write separados | Kill/maintenance exigem grant administrativo | ADMIN-005 |
| MCP-012 | P0 | P | Audit de todas as chamadas | Origin/tool/target/result são registrados | SEC-015 |
| MCP-013 | P0 | A | Output truncation e pagination consistentes | Tool nunca retorna dataset ilimitado | SEC-007 |
| MCP-014 | P0 | P | Cancellation protocolar | Cancel interrompe operação correlacionada | TASK-004 |
| MCP-015 | P0 | A | Production fixture prohibition expandida | Tests rejeitam targets de produção reais | MCP-004 |
| MCP-016 | P0 | A | Protocol tests por nova tool | Schema, grants, limits e errors são cobertos | QUALITY-009 |
| MCP-017 | P1 | A | Resources/prompts não vazam SQL sensível | Scope e redaction são testados | SEC-002 |
| MCP-018 | P1 | A | Revogação imediata durante operação | Nova escrita é bloqueada; running segue policy explícita | MCP-004 |
| MCP-019 | P1 | A | Multi-connection routing explícito | Tool não usa conexão ativa implícita | BASE |
| MCP-020 | P1 | A | Versionar schemas de tool | Mudança incompatível é detectada em test | QUALITY-015 |

## 23. UX — Terminal-first, acessibilidade e discoverability

### 23.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| UX-001 | P0 | P | Keyboard/mouse/palette convergem em actions | Testes comparam resultado/effects | BASE |
| UX-002 | P0 | P | Keyboard-first, não keyboard-only | Toda ação principal é alcançável sem mouse | BASE |
| UX-003 | P1 | A | Action sheet contextual comum | Substitui menus dispersos e right-click obrigatório | UX-001 |
| UX-004 | P1 | P | Command palette pesquisável | Commands exibem requirements/disabled reason | BASE |
| UX-005 | P1 | A | Ajuda contextual por tela/aba | Atalhos relevantes aparecem sem manual externo | UX-004 |
| UX-006 | P0 | P | Estado não depende de cor | Marker/text funciona em no-color/ASCII | BASE |
| UX-007 | P0 | P | Layout amplo/reduzido/compacto | Funções principais continuam alcançáveis | BASE |
| UX-008 | P0 | A | Empty/loading/offline/error/success por superfície | Não há painel vazio ambíguo | BASE |
| UX-009 | P0 | A | Foco previsível após open/close | Overlay devolve foco ao owner correto | BASE |
| UX-010 | P0 | A | Tab order consistente | Form/action sheet são navegáveis por teclado | BASE |
| UX-011 | P1 | P | Mouse click/scroll/drag onde apropriado | Sem hover como requisito | UX-001 |
| UX-012 | P1 | A | Notificações não intrusivas | Background completion não rouba foco | UX-001 |
| UX-013 | P1 | A | Status bar contextual | Project/db/session/tx/pending/task ficam visíveis | BASE |
| UX-014 | P0 | A | Produção/read-only persistentes | Header/tab/review mostram ambiente | SEC-006 |
| UX-015 | P1 | A | Undo/redo discoverable | Scope local é mostrado | DATA-EDIT-017 |
| UX-016 | P1 | A | Confirmações descrevem consequência | Generic yes/no não serve para destructive | SEC-009 |
| UX-017 | P1 | A | Errors com ação recuperável | Retry/reload/details aparecem quando válidos | QUALITY-006 |
| UX-018 | P1 | A | Progress com cancel | Operação longa mostra stage e target | TASK-003 |
| UX-019 | P1 | A | Preferências de keymap | Default/Vim/Emacs não alteram actions | UX-001 |
| UX-020 | P1 | P | Themes e color depth | Truecolor/256/16/no-color têm snapshots | UX-001 |
| UX-021 | P1 | P | Unicode e ASCII fallback | Borders/icons/text não quebram hit regions | QUALITY-008 |
| UX-022 | P1 | A | Screen reader/text export de estado atual | Informação crítica pode ser copiada como texto | UX-006 |
| UX-023 | P1 | A | Search/filter inputs com histórico | Histórico é escopado e limpável | FILES-015 |
| UX-024 | P1 | A | Multi-select claramente separado de cursor | Actions mostram count selecionado | GRID-001 |
| UX-025 | P1 | A | Dirty/pending markers em tabs | Fechamento nunca surpreende | DATA-EDIT-016 |
| UX-026 | P1 | A | Breadcrumb e source context | Usuário sabe projeto/db/schema/object | NAV-022 |
| UX-027 | P1 | A | Clipboard feedback real | Success só após OS clipboard confirmar | GRID-022 |
| UX-028 | P1 | A | Form validation por campo | Erro preserva input e foco | OBJECT-034 |
| UX-029 | P1 | A | Onboarding database-first | Primeiro uso não força query SQL | BASE |
| UX-030 | P0 | A | Accessibility checklist por release | Regressões bloqueiam release | UX-001 |

## 24. PERF — Streaming, memória, latência e limites

### 24.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| PERF-001 | P0 | P | Primeiro frame sem rede | Benchmark usa somente estado local | BASE |
| PERF-002 | P0 | P | Introspecção lazy | Startup não percorre catálogo inteiro | NAV-001 |
| PERF-003 | P0 | A | Índice local de metadata eficiente | Busca em 100k objetos permanece interativa | NAV-011 |
| PERF-004 | P0 | P | Query rows streaming | Memória não cresce com dataset completo | TASK-001 |
| PERF-005 | P0 | P | Backpressure driver -> app -> adapter | Producer desacelera sem OOM | TASK-001 |
| PERF-006 | P0 | P | Grid virtualizado | Render aloca apenas viewport | GRID-003 |
| PERF-007 | P0 | A | Budget de memória por aba/result | Exceder budget faz spool/eviction controlado | BASE |
| PERF-008 | P1 | A | Eviction de abas inativas | Contexto permanece restaurável | BASE |
| PERF-009 | P0 | P | LOB limitado e incremental | Valor grande não materializa silenciosamente | GRID-032 |
| PERF-010 | P0 | A | Limite de concorrência | Global/connection/task queue são configuráveis | TASK-028 |
| PERF-011 | P1 | A | Throttle de progress/render | Milhares de updates não saturam event loop | TASK-003 |
| PERF-012 | P1 | A | Retenção limitada de logs/history | Cleanup evita crescimento ilimitado | QUERY-017 |
| PERF-013 | P0 | A | Data compare externo/streaming | Dataset maior que RAM termina dentro do budget | COMPARE-019 |
| PERF-014 | P1 | A | Large graph lazy/filterable | ERD não renderiza todos os nodes de uma vez | DIAGRAM-003 |
| PERF-015 | P0 | P | Cleanup de temp/spool | Cancel/crash não deixa artefato permanente | SEC-028 |
| PERF-016 | P0 | A | Cancelamento com deadline | Operação que ignora cancel vira erro observável | TASK-004 |
| PERF-017 | P1 | A | Timeout por fase de conexão/query/I/O | Error identifica fase expirada | BASE |
| PERF-018 | P1 | A | Page/cache size configuráveis | Setting mostra trade-off memória/roundtrip | GRID-005 |
| PERF-019 | P1 | A | Export/import streaming | Arquivo grande não é lido inteiro | TRANSFER-003 |
| PERF-020 | P1 | A | Backup process I/O streaming | stdout/stderr são limitados e sanitizados | TRANSFER-028 |
| PERF-021 | P0 | A | Bench first frame | Regressão acima do budget bloqueia release | QUALITY-011 |
| PERF-022 | P0 | A | Bench grid viewport | 100k rows e wide columns têm budget | QUALITY-011 |
| PERF-023 | P1 | A | Bench catalog search | 100k/1M metadata docs têm baseline | QUALITY-011 |
| PERF-024 | P1 | A | Bench transfer/compare | Throughput e peak RSS são registrados | QUALITY-011 |
| PERF-025 | P0 | A | Stress concurrency/cancel | Sem deadlock, leak ou cross-operation event | QUALITY-010 |

## 25. DRIVER — Contratos, PostgreSQL/MySQL e expansão futura

### 25.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| DRIVER-001 | P0 | P | Capability descriptor estruturado | Available/unavailable inclui reason | BASE |
| DRIVER-002 | P0 | A | Matriz de capacidade por sessão/version | UI reflete servidor conectado, não suposição estática | DRIVER-001 |
| DRIVER-003 | P0 | P | Dialeto específico preservado | Core não normaliza SQL ao menor denominador | BASE |
| DRIVER-004 | P1 | P | Metadata driver-specific namespaced | Object editor expande sem quebrar core | BASE |
| DRIVER-005 | P0 | P | Quoting/render DDL/DML por driver | Identifiers e plans passam fixtures adversariais | BASE |
| DRIVER-006 | P1 | A | Type codec/validator por driver | Edit/paste/import compartilham conversão | META-016 |
| DRIVER-007 | P1 | A | Upsert capability | PostgreSQL/MySQL expõem semântica real | BASE |
| DRIVER-008 | P1 | A | Type compatibility mapping cross-driver | Lossy/unsupported são explícitos | BASE |
| DRIVER-009 | P1 | P | Native tool descriptor | Version/path/capability de dump/restore são detectados | BASE |
| DRIVER-010 | P1 | P | Administration provider profundo | Sessions/locks/stats/actions declaram suporte | BASE |
| DRIVER-011 | P1 | P | Explain provider específico | Format/options são preservados | BASE |
| DRIVER-012 | P0 | P | Compatibility matrix de versões | Supported/unverified/unsupported são publicados | BASE |
| DRIVER-013 | P0 | A | Live tests por versão suportada | CI containers cobrem matriz oficial | DRIVER-012 |
| DRIVER-014 | P0 | A | Contract conformance suite | Todo driver oficial passa casos comuns | BASE |
| DRIVER-015 | P1 | A | Error mapping com native code/position/retryable | Adapter recebe categoria consistente | QUALITY-006 |
| DRIVER-016 | P1 | A | Session capability changes observáveis | Reconnect/version change atualiza UI/cache | DRIVER-002 |
| DRIVER-017 | P1 | A | Eventos server-side quando disponíveis | Capability opcional não vira polling obrigatório | DRIVER-001 |
| DRIVER-018 | P3 | F | Gate para novo driver | Contratos estáveis e matriz/testes são obrigatórios | DRIVER-014 |
| DRIVER-019 | P3 | F | Nenhum novo driver superficial | Fluxos P0 aplicáveis precisam estar completos | DRIVER-018 |
| DRIVER-020 | P3 | F | Derivados MariaDB/Postgres avaliados separadamente | Compatibilidade não é assumida por protocolo | DRIVER-018 |
| DRIVER-021 | P0 | A | Fuzz quoting/type/filter | Inputs adversariais não escapam contrato | QUALITY-004 |
| DRIVER-022 | P0 | A | Testar rollback/implicit commit | Semântica DDL/mutação é documentada e verificada | OBJECT-038 |

## 26. QUALITY — Testes, compatibilidade, migrações e release gates

### 26.1 Requisitos

| ID | Pri | Atual | Requisito | Aceitação | Depende |
| --- | --- | --- | --- | --- | --- |
| QUALITY-001 | P0 | P | Unit tests de domínio | Invariantes não dependem de TUI | BASE |
| QUALITY-002 | P0 | P | Integration tests app/storage | Services e repositories são exercitados juntos | BASE |
| QUALITY-003 | P0 | A | Driver conformance suite | Mesmos casos P0 rodam nos dois drivers | DRIVER-014 |
| QUALITY-004 | P0 | P | Property/fuzz tests | Parser/quoting/config/transfer/change set têm corpora | BASE |
| QUALITY-005 | P0 | P | Storage migration tests | Upgrade cria backup e preserva dados | BASE |
| QUALITY-006 | P0 | A | Failure injection | Disk/network/permission/driver errors preservam estado | BASE |
| QUALITY-007 | P0 | P | TUI state-flow tests | Action -> model/effects é determinístico | UX-001 |
| QUALITY-008 | P0 | P | Snapshot tests por capability/layout | Truecolor/low/no-color/ASCII/compact passam | UX-020 |
| QUALITY-009 | P0 | P | CLI golden/protocol tests | Help/JSON/errors permanecem compatíveis | BASE |
| QUALITY-010 | P0 | A | Concurrency/correlation stress | Nenhum evento cruza operação/sessão/generation | TASK-002 |
| QUALITY-011 | P0 | P | Performance baselines | Regressão fora do budget falha CI/gate | BASE |
| QUALITY-012 | P1 | A | Deterministic artifact tests | DDL/diff/diagram/report têm output estável | BASE |
| QUALITY-013 | P0 | A | Crash/recovery tests | Estado local volta; mutação não reexecuta | SEC-029 |
| QUALITY-014 | P0 | P | External change/drift tests | Arquivo/schema alterado invalida preview | FILES-008 |
| QUALITY-015 | P1 | A | Versioned serialization contract tests | Future/unknown version falha de modo seguro | BASE |
| QUALITY-016 | P0 | P | Docs examples tested | Commands e claims correspondem ao binário | BASE |
| QUALITY-017 | P0 | P | Security/threat tests | Secrets, path traversal, injection e grants são cobertos | SEC-025 |
| QUALITY-018 | P0 | P | Recovery repository tests | Dirty docs/layout e futuras edits restauram corretamente | BASE |
| QUALITY-019 | P0 | P | MCP grants/protocol/adversarial tests | Least privilege e production prohibition passam | MCP-015 |
| QUALITY-020 | P0 | P | Release artifact verification | Checksums/SBOM/installers são validados | BASE |
| QUALITY-021 | P0 | P | Live PostgreSQL/MySQL tests | Operações dependentes do servidor não ficam ignored no gate | DRIVER-013 |
| QUALITY-022 | P0 | P | Linux/macOS/Windows CI | Paths/terminal/keychain behavior aplicável passa | BASE |
| QUALITY-023 | P0 | A | Accessibility checklist gate | Keyboard/no-color/ASCII/focus são revisados por release | UX-030 |
| QUALITY-024 | P0 | P | `cargo fmt --check` | Workspace formatado | BASE |
| QUALITY-025 | P0 | P | Clippy `-D warnings` all targets | Zero warning no gate | BASE |
| QUALITY-026 | P0 | P | `cargo test --workspace --all-targets` | Suíte não-live passa | BASE |
| QUALITY-027 | P0 | P | Dependency/license/advisory checks | Policy de supply chain passa | BASE |
| QUALITY-028 | P0 | A | Requirement coverage report | IDs entregues apontam para testes e docs | BASE |
| QUALITY-029 | P0 | A | No orphan engine gate | Motor novo precisa de owner/adapter/plan explícito | BASE |
| QUALITY-030 | P0 | A | Manual exploratory checklist proporcional | Fluxos destrutivos e terminal variants são cobertos | UX-030 |

## 27. Fluxos verticais obrigatórios

Os fluxos abaixo são critérios de produto. Um conjunto de componentes isolados não os
substitui.

### FLOW-001 — Visualizar dados sem SQL

```text
Start Center
-> projeto
-> database
-> schema
-> tabela
-> TableData
-> filter/sort/page
```

Aceitação:

- nenhum console SQL é criado;
- cache aparece antes da rede quando disponível;
- conexão e loading não bloqueiam a UI;
- erros preservam a navegação e o contexto.

### FLOW-002 — Editar uma célula com segurança

```text
TableData
-> metadata editável
-> editar célula
-> validar
-> change set local
-> review original/novo/DML
-> apply atômico
-> affected == 1
-> reload preservando contexto
```

Aceitação:

- nada é enviado antes de Apply;
- valor inválido não chega ao driver;
- falha preserva a mudança local;
- conflito oferece reload, keep local e revert.

### FLOW-003 — Inserir, clonar e excluir

```text
TableData
-> add/clone/delete
-> defaults/generated respeitados
-> pending markers
-> review
-> apply
```

Aceitação:

- insert não envia null no lugar de default omitido;
- clone limpa identity/generated;
- delete usa identidade estável e affected-row check.

### FLOW-004 — Bulk edit/delete/paste

```text
multi-select
-> transformação ou delete/paste
-> materializar identities
-> count + limits + environment
-> preview
-> atomic/chunk policy
-> progress/conflicts/report
```

Aceitação:

- sort/filter após a seleção não muda o alvo confirmado;
- produção exige confirmação reforçada;
- cancelamento relata rollback ou commits parciais com precisão.

### FLOW-005 — Operar tabela sem chave

```text
TableData read-only
-> reason: no stable identity
-> Choose Row Identifier
-> escolher colunas
-> validar metadata
-> habilitar edição
```

Aceitação:

- read-only é o default;
- escolha é explícita e revogável;
- todo update/delete ainda exige affected == 1;
- mudança de schema invalida a escolha incompatível.

### FLOW-006 — Criar ou alterar objeto

```text
Navigator/ObjectEditor
-> formulário tipado
-> validate
-> DDL plan + risk + locks
-> preview
-> confirm
-> apply
-> refresh affected subtree
```

Aceitação:

- alteração do formulário invalida preview antigo;
- produção e ações destrutivas usam confirmação forte;
- erro preserva formulário e plano para correção.

### FLOW-007 — Buscar e investigar objeto

```text
global search
-> cache local
-> optional online enrichment
-> result with connection/schema/type/source
-> ObjectEditor
-> dependencies/find usages/DDL/data
```

Aceitação:

- busca é cancelável;
- resultado cacheado é identificado;
- objeto restricted permanece navegável com limitação explícita.

### FLOW-008 — Transferir dados entre databases

```text
source table/query
-> destination connection/table
-> column/type mapping
-> sample preview
-> create target optional DDL review
-> batch streaming
-> progress/cancel
-> verification report
```

Aceitação:

- PostgreSQL/MySQL cross-transfer declara conversões e perdas;
- nenhum secret aparece em task/log/argv;
- resultado parcial é distinguido de sucesso completo.

### FLOW-009 — Comparar e sincronizar dados

```text
source + target
-> map columns/key
-> streaming compare
-> only-source/only-target/changed
-> select direction
-> mutation preview
-> protected apply
```

Aceitação:

- relatório pode ser exportado sem aplicar;
- direção de sync nunca é implícita;
- conflito ou duplicate key não gera resultado silenciosamente incorreto.

### FLOW-010 — Executar tarefa salva

```text
task definition
-> resolve non-secret variables
-> acquire secret at runtime
-> review target
-> queue/run
-> progress/log
-> completed/failed/cancelled artifact
```

Aceitação:

- definição não persiste segredo;
- retry segue policy de idempotência;
- crash nunca reexecuta write automaticamente.

### FLOW-011 — Investigar blocking session

```text
Admin
-> sessions/locks
-> blocking graph
-> inspect blocker/waiter
-> cancel or terminate preview
-> privilege/policy check
-> confirm
-> audit + refresh
```

Aceitação:

- cancel e terminate são ações distintas;
- target e privilégio ficam visíveis;
- falha de permissão não derruba o painel.

### FLOW-012 — Usar offline e reconectar

```text
open project offline
-> cached navigator/object metadata
-> open stale tabs
-> connect on demand
-> reconcile metadata
-> preserve local state
```

Aceitação:

- first frame não espera rede;
- metadata divergente bloqueia edição até refresh;
- reconexão não executa write pendente.

### FLOW-013 — Automatizar via CLI

```text
dexo <operation> --non-interactive
-> explicit connection/project/target
-> policy and limits
-> structured progress/result
-> deterministic exit code
```

Aceitação:

- comando não prompta em non-interactive;
- write/destructive exige confirm flag ou artefato de plano aplicável;
- JSON não mistura logs humanos em stdout.

### FLOW-014 — Expor capacidade via MCP

```text
profile + allowlist
-> temporary scoped grant
-> tool call with explicit connection/target
-> app service + policy
-> limits/cancel
-> audit
-> grant expiry/revoke
```

Aceitação:

- MCP não cria grant;
- read grant não permite write;
- tool não usa sessão ativa implícita;
- produção real é proibida nas fixtures.

## 28. Gates do programa

Os gates definem dependências, não datas ou releases.

### GATE-0 — Workspace database-first

Pré-requisito externo definido na spec-base. Deve entregar projeto, navigator, abas
tipadas, cache-first e SQL opcional.

### GATE-1 — Contratos de editabilidade

Obrigatório antes de qualquer UI de edição:

- META-001 a META-012;
- DRIVER-001, DRIVER-002, DRIVER-005, DRIVER-006;
- conformance e live tests de metadata.

Saída: `TableData` consegue explicar de forma confiável se e como é editável.

### GATE-2 — Safe data editing

Obrigatório antes de bulk:

- DATA-EDIT P0;
- SEC P0 aplicável;
- FLOW-002, FLOW-003 e FLOW-005;
- testes live PostgreSQL/MySQL.

Saída: edição individual, insert, clone e delete seguros e utilizáveis.

### GATE-3 — Bulk e grid profissional

- BULK P0;
- GRID P0 e P1 necessário ao fluxo;
- performance/selection/cancel tests;
- FLOW-004.

Saída: operações em lote e data browsing cobrem o trabalho diário.

### GATE-4 — Navigator, objetos e produtividade

- NAV, OBJECT, FILES e QUERY P1;
- object operations protegidas;
- query manager e busca global;
- FLOW-006 e FLOW-007.

Saída: exploração e manutenção de schema não dependem de SQL manual.

### GATE-5 — Operações profissionais

- TRANSFER, COMPARE e ADMIN;
- DIAGRAM read-only/exportável;
- streaming e background runtime maduros;
- FLOW-008, FLOW-009 e FLOW-011.

Saída: migração, comparação, diagnóstico e administração formam fluxos completos.

### GATE-6 — Tasks e automação governada

- TASK definitions/composite/external scheduler;
- CLI estruturada;
- MCP grants/limits/audit para novas capacidades;
- FLOW-010, FLOW-013 e FLOW-014.

Saída: operações podem ser repetidas e automatizadas sem daemon ou segredo persistido.

### GATE-7 — Paridade profissional PostgreSQL/MySQL

- todos os P0 e P1 aplicáveis concluídos;
- matriz de drivers sem lacunas silenciosas;
- live compatibility matrix verde;
- performance, accessibility, security e release gates verdes;
- documentação e coverage report atualizados.

Somente após GATE-7 novos drivers podem avançar de experimento para suporte oficial.

## 29. Matriz de rastreabilidade por domínio

| Domínio | IDs | Fluxos principais | Gate de entrada | Evidência mínima |
| --- | --- | --- | --- | --- |
| Metadata | META-001..024 | FLOW-002, FLOW-005, FLOW-012 | GATE-0 | Unit + conformance + PG/MySQL live |
| Edição | DATA-EDIT-001..040 | FLOW-002, FLOW-003 | GATE-1 | App/TUI + conflict + PG/MySQL live |
| Bulk | BULK-001..025 | FLOW-004 | GATE-2 | Selection + policy + cancel + live |
| Grid | GRID-001..040 | FLOW-001..005 | GATE-0 | State/snapshot/perf/live fetch |
| Navigator | NAV-001..030 | FLOW-001, FLOW-007, FLOW-012 | GATE-0 | Cache/search/mouse/perf |
| Objects | OBJECT-001..040 | FLOW-006, FLOW-007 | GATE-1 | Planner + UI + DDL live |
| Files | FILES-001..025 | FLOW-007, FLOW-010 | GATE-0 | Storage/filesystem/cross-platform |
| Query | QUERY-001..030 | FLOW-001, FLOW-013 | GATE-0 | Runtime/correlation/privacy/live |
| Transfer | TRANSFER-001..035 | FLOW-008, FLOW-010 | GATE-3 | Codec/stream/cancel/cross-driver live |
| Compare | COMPARE-001..028 | FLOW-009 | GATE-3 | Deterministic diff/stream/sync live |
| Diagram | DIAGRAM-001..016 | FLOW-007 | GATE-4 | Graph/layout/export fixtures |
| Tasks | TASK-001..028 | FLOW-010, FLOW-013 | GATE-0 | Lifecycle/recovery/concurrency/CLI |
| Admin | ADMIN-001..028 | FLOW-011 | GATE-4 | Privilege/policy/PG/MySQL live |
| Security | SEC-001..030 | Todos os mutáveis | GATE-0 | Threat/property/adapter parity |
| MCP | MCP-001..020 | FLOW-014 | GATE-2 por write | Protocol/grants/limits/audit |
| UX | UX-001..030 | Todos os interativos | GATE-0 | Keyboard/mouse/snapshot/a11y |
| Performance | PERF-001..025 | Todos os grandes | GATE-0 | Baselines/stress/RSS/cleanup |
| Drivers | DRIVER-001..022 | Todos os server-side | GATE-0 | Conformance/version matrix/live |
| Quality | QUALITY-001..030 | Todos | GATE-0 | Coverage report e release gates |

## 30. Matriz de paridade de tarefas

| Tarefa profissional | Dexo alvo | Requisitos principais |
| --- | --- | --- |
| Navegar databases e schemas | Navigator database-first e cache-first | NAV-001..010, BASE |
| Buscar objeto/coluna/DDL | Busca global local + online | NAV-011..015 |
| Abrir tabela sem SQL | `TableData` tipada | FLOW-001, GRID-004 |
| Editar/adicionar/clonar/excluir | Change set seguro | DATA-EDIT-001..040 |
| Bulk edit/delete/paste | Bulk plan + limits | BULK-001..025 |
| Filtrar/ordenar/buscar dados | Grid local/remoto explícito | GRID-008..016 |
| Record/value editor | Record view e viewers especializados | GRID-029..038 |
| Navegar registros relacionados | FKs nos dois sentidos | META-013..014, GRID-034..035 |
| Inspecionar/alterar objeto | Object editor + DDL planner | OBJECT-001..040 |
| Gerar SQL | `SqlGenerator` específico | OBJECT-017..021 |
| Gerenciar arquivos e snippets | File tree + productivity | FILES-001..025 |
| Revisar histórico e operações | Query manager | QUERY-012..020 |
| Exportar/importar/migrar | Transfer runtime streaming | TRANSFER-001..035 |
| Comparar schemas | Schema diff/sync protegido | COMPARE-001..012 |
| Comparar/sincronizar dados | Data compare streaming | COMPARE-013..028 |
| Visualizar relações | ERD textual + Mermaid/Graphviz | DIAGRAM-001..016 |
| Salvar e repetir operações | Task definitions/composite | TASK-011..022 |
| Investigar sessions/locks | Admin + blocking graph | ADMIN-001..014 |
| Maintenance/explain | Driver-specific admin | ADMIN-015..028 |
| Automatizar por CLI | Structured non-interactive adapters | FLOW-013, TASK-023 |
| Expor a assistentes locais | MCP least-privilege | MCP-001..020 |

## 31. Modelo de erros e recuperação

### 31.1 Categorias

```text
Configuration
Authentication
Authorization
Network
Timeout
Cancelled
Unsupported
Validation
Conflict
Constraint
Server
Storage
ExternalChange
UnknownOutcome
```

Todo erro deve informar, quando aplicável:

- categoria estável;
- mensagem sanitizada;
- código nativo;
- posição/campo/linha;
- target e operation ID;
- se retry pode ser seguro;
- ação de recuperação disponível.

### 31.2 Regras

- `Cancelled` não é sucesso nem erro genérico.
- `UnknownOutcome` após perda de conexão em write exige reconciliação manual.
- `Conflict` preserva state local e oferece resolução.
- `Unsupported` desabilita capacidade com razão; não vira falha repetitiva.
- `Storage` não confirma persistência que não ocorreu.
- `ExternalChange` invalida preview/dirty base antes de sobrescrever.
- Erro de uma aba/nó/task não derruba o workspace.

## 32. Persistência, recovery e privacidade

### 32.1 Durável

- projetos, escopos de conexão e layouts;
- abas/contexto não sensível;
- filtros, column layouts, favoritos e recentes;
- snippets, task definitions e presets;
- metadata/cache reconstruível;
- histórico/audit conforme retenção configurada.

### 32.2 Recuperável após crash

- documentos dirty;
- estado local de change sets não aplicados;
- formulários e previews como não confirmados;
- task state como failed/unknown quando aplicável;
- nunca uma confirmação ou grant expirado.

### 32.3 Proibido persistir silenciosamente

- senhas, tokens, private keys e secret parameters;
- result sets completos;
- clipboard;
- transações ou handles de sessão;
- confirmação de produção;
- autorização MCP além de sua expiração explícita.

## 33. Estratégia de specs e planos derivados

Esta spec deve gerar múltiplos ciclos spec -> plan -> implementation. A decomposição
inicial recomendada é:

1. Metadata editável e row identity.
2. Edição individual, insert, clone e delete.
3. Bulk edit/delete/paste.
4. Grid profissional e record/value editor.
5. Busca global e explorer avançado.
6. Object editor e SQL generator.
7. File tree, favoritos e query manager.
8. Transferência table-to-table e presets.
9. Data compare e sync.
10. Diagramas read-only/exportáveis.
11. Background task center e task definitions.
12. Administração e monitoring.
13. MCP/CLI exposure das capacidades concluídas.
14. Hardening de performance, accessibility e release.

Uma spec derivada pode agrupar itens quando o fluxo continuar verificável. Ela não
pode combinar todos os domínios em um único plano nem implementar UI antes dos
contratos dos quais depende.

## 34. Critérios de conclusão por requisito

Um requisito só muda para completo quando:

1. comportamento e edge cases estão implementados;
2. adapter e action surface aplicáveis estão alcançáveis;
3. empty/loading/error/offline/success foram tratados;
4. políticas read-only/produção/limites foram aplicadas;
5. documentação pública não promete além da implementação;
6. testes definidos pelo requisito passam;
7. live test existe quando o servidor participa;
8. performance e cleanup estão dentro dos budgets;
9. requirement coverage report aponta commits/testes/docs;
10. não há dependência obrigatória ainda incompleta.

## 35. Critérios de aceitação do programa

O programa de paridade profissional PostgreSQL/MySQL está concluído somente quando:

1. todos os requisitos P0 aplicáveis estão completos;
2. todos os requisitos P1 aplicáveis estão completos ou possuem exceção aprovada e
   documentada por capability;
3. FLOW-001 a FLOW-014 passam em seus adapters aplicáveis;
4. PostgreSQL e MySQL passam a matriz live de versões suportadas;
5. TUI inicia e opera database-first sem exigir SQL;
6. edição individual e bulk são seguras, revisáveis e recuperáveis;
7. explorer, object editor e grid cobrem o trabalho diário;
8. transfer, compare, tasks e admin funcionam como fluxos verticais;
9. CLI e MCP reutilizam services/policies, sem implementações paralelas;
10. no-color, ASCII, layouts compactos e plataformas oficiais passam;
11. performance baselines e stress/cancellation gates passam;
12. secrets/threat/audit tests passam;
13. docs, compatibility matrix e coverage report correspondem ao produto;
14. nenhuma feature é declarada completa apenas por possuir um motor interno.

## 36. Fora de escopo até revisão desta spec

- Conta, cloud sync ou backend obrigatório.
- Colaboração multiusuário em tempo real.
- Telemetria obrigatória.
- Armazenamento de segredos fora do keychain/prompt de sessão.
- Servidor MCP HTTP ou daemon de rede.
- Cliente MCP embutido.
- Modelo/chat de IA embutido.
- Marketplace de plugins e ABI externa de drivers.
- GUI desktop/web.
- Edição visual de schema como requisito P0/P1.
- Drivers adicionais antes de GATE-7.
- Suporte oficial presumido a MariaDB ou derivados PostgreSQL.
- Execução automática de migração/sync gerado sem review.
- Retry automático de operações mutáveis.
- Persistência de datasets completos como parte do workspace.

## 37. Riscos e respostas arquiteturais

| Risco | Consequência | Resposta obrigatória |
| --- | --- | --- |
| Big bang de funcionalidades | Motores parciais e UX fragmentada | Specs pequenas por gate e fluxo vertical |
| Estado global da TUI | Abas interferem entre si | Estado pertence à `WorkspaceTab` |
| Abstração pelo menor denominador | Perda de profundidade PostgreSQL/MySQL | Capabilities e extensões driver-specific |
| Metadata obsoleta | Update/delete na linha errada | Identity + metadata reconciliation + affected check |
| Bulk em produção | Grande impacto acidental | Limits, count, typed confirmation e preview |
| Retry após falha de rede | Duplicação ou estado desconhecido | `UnknownOutcome` e reconciliação manual |
| Dataset grande | OOM/TUI travada | Streaming, backpressure, spool, budgets |
| Tasks persistindo segredos | Vazamento local | Secret placeholders e runtime resolution |
| MCP ampliando escopo | Write não autorizado | Grants externos, temporários e auditados |
| Muitos drivers cedo | Contratos frágeis e suporte raso | PostgreSQL/MySQL até GATE-7 |
| Paridade visual desktop | TUI complexa e inconsistente | Equivalência de tarefas terminal-first |
| Testes somente unitários | Falsa completude | Live vertical tests e coverage report |

## 38. Decisões finais

1. A spec-base database-first é fundação obrigatória.
2. Esta é a especificação-mãe estável do pós-workspace.
3. PostgreSQL e MySQL são o alvo de paridade profissional.
4. Novos drivers são expansão futura após GATE-7.
5. Segurança usa change set local, review explícito e apply protegido.
6. Tabelas sem identidade são read-only até escolha manual governada.
7. Bulk é atômico por padrão; chunks são explícitos e potencialmente parciais.
8. DataGrip/DBeaver são referências de tarefa, não de layout.
9. TUI, CLI e MCP compartilham domínio, policies e drivers.
10. Cada plano futuro deve citar IDs deste documento e entregar evidência vertical.
