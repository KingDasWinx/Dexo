# Dexo — Especificação de Produto e Design Técnico

**Status:** aprovado para planejamento

**Data:** 2026-08-14

**Licença planejada:** MIT OR Apache-2.0

**Plataformas oficiais:** Linux, macOS e Windows

## 1. Resumo executivo

Dexo é um gerenciador e visualizador local de bancos de dados executado no terminal. Ele combina uma TUI rica, voltada ao uso interativo, com uma CLI não interativa adequada a scripts e automações. O produto busca paridade ampla com os fluxos profissionais centrais de ferramentas como DataGrip: conexão, exploração, edição SQL, visualização e alteração de dados, engenharia de schemas, diagnóstico e administração.

A primeira geração terá suporte oficial a PostgreSQL e MySQL. O sistema será um Cargo workspace modular, com drivers oficiais compilados no binário e contratos orientados a capacidades. Essa separação permitirá adicionar bancos futuramente sem acoplar a interface, o executor ou o armazenamento local aos detalhes de um protocolo.

Dexo é local-first: não exige conta, servidor próprio ou cloud. Configurações, histórico, snippets, cache de catálogo e layouts permanecem na máquina. Segredos são guardados exclusivamente pelo keychain nativo do sistema operacional.

## 2. Objetivos

1. Entregar uma experiência de banco de dados completa sem sair do terminal.
2. Manter a interface responsiva durante conexões, consultas, exportações e introspecções longas.
3. Oferecer profundidade real em PostgreSQL e MySQL, sem reduzir ambos a um subconjunto SQL genérico.
4. Permitir que um novo banco oficial seja implementado como crate independente atrás de contratos estáveis.
5. Proteger o usuário contra operações destrutivas acidentais com políticas configuráveis e contexto visível.
6. Compartilhar o mesmo domínio e casos de uso entre TUI e CLI.
7. Funcionar de forma nativa e consistente em Linux, macOS e Windows.
8. Ser distribuído como software open source permissivo, auditável e sem telemetria obrigatória.

## 3. Não objetivos

- Contas, cloud sync, workspaces remotos ou colaboração em tempo real.
- Armazenamento de credenciais fora do keychain do sistema.
- Interface gráfica desktop ou web.
- Drivers externos ou plugins de terceiros na primeira arquitetura.
- Compatibilidade perfeita com todo recurso visual ou periférico do DataGrip.
- Reimplementar mecanismos de backup já fornecidos pelos bancos.
- Executar migrações geradas sem revisão explícita por padrão.
- Oferecer um servidor intermediário obrigatório entre o usuário e o banco.
- Implementar controle de versão próprio; arquivos SQL continuam compatíveis com Git e outras ferramentas existentes.

## 4. Princípios de produto

### 4.1 Local-first

Todas as funções principais operam sem uma conta Dexo. Recursos que dependem do banco exigem apenas acesso à conexão configurada. Catálogo em cache, documentos recentes e snippets continuam disponíveis offline.

### 4.2 Keyboard-first, não keyboard-only

Toda ação deve ser alcançável por teclado e pela command palette. Mouse é opcional para seleção, foco, scroll e redimensionamento. Perfis de teclas Vim e Emacs são opcionais; o perfil padrão usa convenções comuns de aplicações de terminal.

### 4.3 Streaming por padrão

Resultados, exportações, introspecção e logs são processados incrementalmente. O tamanho do resultado não pode determinar diretamente o uso de memória da aplicação.

### 4.4 Específico onde importa

Conexão, catálogo, execução e resultados compartilham contratos comuns. Dialeto, DDL, tipos, explain, objetos especiais e administração permanecem extensíveis por driver.

### 4.5 Segurança visível

Ambiente, conexão e estado transacional permanecem visíveis durante qualquer operação mutável. Proteções não dependem apenas de cor nem de um alerta momentâneo.

## 5. Escopo funcional

### 5.1 Projetos e estado local

- Criar, renomear, abrir e remover projetos locais.
- Associar conexões, documentos SQL, snippets, favoritos e layouts a um projeto.
- Restaurar abas e painéis após encerramento normal ou inesperado.
- Abrir e salvar arquivos SQL comuns fora do diretório interno do Dexo.
- Exibir itens recentes e permitir limpar histórico por projeto ou conexão.
- Exportar e importar configurações não sensíveis para backup manual.
- Nunca incluir segredos em exportações de configuração.

### 5.2 Gerenciamento de conexões

- Criar, editar, duplicar, testar, organizar e remover conexões.
- Organizar conexões por grupo, projeto e ambiente.
- Classificar ambientes como local, desenvolvimento, homologação, produção ou rótulo personalizado.
- Configurar host, porta, banco inicial, usuário, parâmetros do driver, timeouts e application name.
- Descobrir versão do servidor e publicar a matriz de capacidades da sessão.
- Suportar TLS com certificados da plataforma, CA personalizada, certificado de cliente e chave privada.
- Exigir validação de certificado por padrão; opções inseguras exibem aviso persistente.
- Suportar SSH tunnel com senha, chave, agent e known hosts.
- Verificar host keys SSH e exigir confirmação explícita para uma chave nova ou alterada.
- Suportar SOCKS5 e HTTP CONNECT para conexões TCP; encadeamentos específicos de SSH ficam sob a capacidade do transporte SSH.
- Reconectar sessões ociosas somente quando a operação for segura.
- Permitir múltiplas sessões independentes para uma mesma conexão.
- Marcar uma conexão ou sessão como somente leitura.
- Testar conexão sem persistir a senha fornecida quando o usuário não autorizar.
- Buscar e gravar segredos exclusivamente pelo keychain nativo.
- Quando o keychain estiver ausente ou bloqueado, solicitar o segredo por sessão e nunca criar um cofre em arquivo como fallback silencioso.

### 5.3 Explorador de banco

- Navegar por servidor, database/catalog, schema e tipos de objeto.
- Carregar a árvore sob demanda, sem introspectar todo o servidor no primeiro frame.
- Atualizar um nó, subtree ou catálogo completo.
- Filtrar objetos por nome, tipo, schema e favorito.
- Pesquisar globalmente com ranking por correspondência e recência.
- Abrir propriedades, DDL, dados, dependências e dependentes de um objeto.
- Copiar nome simples, nome qualificado e DDL.
- Navegar de uma referência SQL ao objeto e de um objeto aos usos conhecidos.
- Exibir privilégios efetivos quando o banco fornecer informação suficiente.
- Manter snapshot local do catálogo para navegação e autocomplete offline.

Objetos comuns incluem tabelas, views, colunas, índices, constraints, chaves, sequences, funções, procedures, triggers, usuários e roles. Objetos exclusivos permanecem sob namespaces de capacidade do driver.

### 5.4 Workbench e editor SQL

- Múltiplas abas e documentos, associados opcionalmente a conexão e database/schema.
- Syntax highlighting incremental e tolerante a SQL incompleto.
- Numeração de linhas, seleção múltipla, busca, substituição, undo e redo.
- Indentação, comment toggle, pareamento de delimitadores e destaque de pares.
- Identificação do statement sob o cursor e execução da seleção, statement ou arquivo completo.
- Autocomplete de keywords, objetos, colunas, aliases, funções, parâmetros e snippets.
- Resolução contextual de aliases, CTEs, subqueries e escopo.
- Signature help e documentação curta para funções conhecidas.
- Navegação para definição e referências quando resolvíveis pelo catálogo.
- Formatação por dialeto e preview antes de substituir o texto.
- Diagnósticos locais diferenciados de erros retornados pelo servidor.
- Quick fixes apenas quando a transformação for determinística e revisável.
- Variáveis nomeadas e posicionais com editor de valores tipados.
- Snippets pessoais com placeholders.
- Histórico pesquisável com duração, status, conexão e timestamp.
- Favoritar, nomear e reabrir consultas.
- Scratch documents recuperáveis sem exigir arquivo em disco.
- Detecção de alterações externas em arquivos abertos.

O parser local ajuda o editor, mas nunca é a autoridade final sobre tudo que o servidor aceita. SQL desconhecido pode ser executado depois das verificações de segurança aplicáveis.

Quando o histórico estiver habilitado, ele armazena o texto SQL porque essa é a função solicitada pelo usuário, mas exclui valores fornecidos pelo editor de parâmetros por padrão. A configuração explica que literais sensíveis escritos diretamente no SQL continuam fazendo parte do documento e permite desabilitar ou limitar a retenção por conexão.

### 5.5 Execução e sessões SQL

- Executar um statement, seleção ou script com múltiplos statements.
- Mostrar cada result set, contagem afetada, notices, warnings e mensagens do servidor.
- Suportar prepared statements quando benéfico e execução direta quando necessária.
- Cancelar consulta por mecanismo específico do servidor.
- Definir timeout, limite inicial de linhas e tamanho máximo de cache.
- Oferecer modo autocommit e transação manual.
- Manter commit, rollback e estado de erro da transação sempre visíveis.
- Criar, liberar e reverter savepoints.
- Impedir troca silenciosa de sessão enquanto uma transação manual estiver aberta.
- Não repetir automaticamente operações mutáveis ou transações interrompidas.
- Exibir duração, tempo até primeira linha, linhas recebidas e taxa de transferência.
- Executar tarefas em background e notificar conclusão sem bloquear a edição.
- Permitir execução sequencial de script; paralelismo exige ação explícita.

### 5.6 Visualização e edição de resultados

- Grade virtualizada, paginada e navegável por teclado ou mouse.
- Largura automática e manual, freeze de colunas e ocultação temporária.
- Ordenação e filtros locais para o cache atual.
- Reexecutar consulta com filtros e ordenação no servidor quando a origem for uma tabela editável.
- Copiar célula, linha, coluna ou seleção como texto, CSV, TSV, JSON, Markdown ou SQL.
- Visualizadores especializados para texto longo, JSON, XML, arrays, datas, binários e imagens detectáveis.
- Preservar distinção entre `NULL`, string vazia, bytes vazios e valores truncados.
- Baixar valores grandes sob demanda e avisar antes de exceder limites configurados.
- Navegar por foreign keys entre registros relacionados.
- Mostrar múltiplos result sets em abas independentes.

Edição de dados usa um change set local:

- Inserções, updates e deletes ficam pendentes até aplicação explícita.
- O usuário pode revisar SQL e valores antes de aplicar.
- Update/delete exige primary key ou identidade única confiável por padrão.
- Tabelas sem identidade confiável abrem em modo somente leitura.
- A quantidade de linhas afetadas é validada.
- Conflitos de concorrência impedem commit silencioso de resultado inesperado.
- Erro em uma alteração preserva as demais alterações pendentes para correção ou descarte.

### 5.7 Engenharia de schemas e objetos

- Criar, alterar, renomear e remover objetos suportados pelo driver.
- Oferecer formulários TUI para propriedades comuns e editor DDL para controle completo.
- Gerar preview do DDL antes de executar.
- Ordenar statements por dependências quando uma operação envolve vários objetos.
- Mostrar impacto conhecido em dependentes antes de alteração destrutiva.
- Gerar DDL de criação e alteração com quoting correto para o dialeto.
- Editar colunas, defaults, identity/auto increment, índices, constraints e foreign keys.
- Editar views, routines, triggers, usuários, roles e grants quando suportado.
- Atualizar o cache de catálogo após DDL bem-sucedido.
- Não prometer preservação de comentários ou formatação textual ao regenerar DDL.

### 5.8 Comparação de schemas e migrações

- Comparar database/schema com outro banco ou snapshot local.
- Filtrar por tipo de objeto e namespace.
- Normalizar diferenças irrelevantes quando o driver comprovar equivalência.
- Preservar diferenças específicas do banco em extensões tipadas.
- Exibir objetos adicionados, removidos e alterados.
- Calcular ordem por dependências e sinalizar ciclos ou operações manuais.
- Gerar script de migração revisável em ambas as direções quando possível.
- Marcar operações destrutivas, perda potencial de dados e locks esperados.
- Salvar snapshot e relatório localmente.
- Aplicar script apenas depois de revisão e confirmação de política.

### 5.9 Importação, exportação, backup e restore

- Importar CSV, TSV, JSON, JSONL e SQL para tabela nova ou existente.
- Detectar encoding, delimitador, cabeçalho e tipos, permitindo correção manual.
- Mapear colunas e mostrar preview antes da escrita.
- Escolher estratégia de erro: parar, pular linha ou registrar rejeitados.
- Executar importação em lotes, com progresso e cancelamento.
- Exportar consulta ou tabela para CSV, TSV, JSON, JSONL e SQL.
- Escrever exportações por streaming e usar arquivo temporário seguido de rename atômico.
- Controlar representação de `NULL`, datas, binários e escaping.
- Integrar opcionalmente com `pg_dump`, `pg_restore` e ferramentas MySQL compatíveis.
- Detectar ferramenta, versão e argumentos antes da execução.
- Nunca passar senha na linha de comando; usar mecanismo seguro suportado pela ferramenta.
- Exibir stdout/stderr sanitizados e permitir cancelar o subprocesso.

### 5.10 Explain e análise de consultas

- Executar explain simples sem executar a consulta quando o banco permitir.
- Exigir confirmação adicional para variantes que realmente executam o statement.
- Solicitar formato estruturado, preferencialmente JSON, quando disponível.
- Renderizar árvore, tabela e resumo textual.
- Mostrar custo, cardinalidade estimada/real, tempo, loops e operações relevantes.
- Destacar divergências relevantes entre estimado e real sem apresentar heurística como certeza.
- Copiar ou exportar plano bruto e renderizado.
- Comparar dois planos salvos do mesmo dialeto.

### 5.11 Diagnóstico e administração

- Listar sessões, usuário, database, estado, duração e statement atual conforme permissões.
- Listar queries ativas, locks e relações de bloqueio.
- Cancelar query ou encerrar sessão com confirmação proporcional ao risco.
- Mostrar tamanho de databases, schemas, tabelas e índices.
- Mostrar estatísticas expostas pelo banco e timestamp da coleta.
- Visualizar variáveis/configurações e distinguir valores de sessão e servidor.
- Executar ações de manutenção suportadas, como analyze/vacuum/reindex ou equivalentes, com preview e proteção.
- Gerenciar usuários, roles e grants quando autorizado.
- Não esconder limitações causadas por versão ou permissão; a capability deve explicar a indisponibilidade.

### 5.12 CLI híbrida

A CLI reutiliza os mesmos casos de uso da TUI. A árvore pública planejada inclui:

```text
dexo                         abre a TUI
dexo connections list|test
dexo query
dexo run
dexo inspect
dexo export
dexo import
dexo schema snapshot|diff
dexo explain
dexo sessions list|cancel|terminate
dexo config show|path
dexo completion
```

Contratos da CLI:

- Aceitar SQL por argumento, arquivo ou stdin conforme o comando.
- Aceitar parâmetros separados do texto SQL.
- Separar stdout de dados e stderr de diagnóstico/progresso.
- Oferecer formatos `table`, `csv`, `tsv`, `json` e `jsonl` quando aplicáveis.
- Produzir códigos de saída estáveis por categoria de erro.
- Nunca abrir prompt quando `--non-interactive` estiver ativo.
- Recusar confirmação destrutiva em modo não interativo sem flag explícita adequada.
- Respeitar ausência de TTY e desabilitar cores/progresso animado automaticamente.
- Disponibilizar completions para shells suportados pelo Clap.

### 5.13 Personalização e acessibilidade

- Temas claros, escuros e adaptados a 16/256/true color.
- Cores nunca são o único indicador de ambiente, erro ou seleção.
- Atalhos reconfiguráveis com detecção de conflitos.
- Command palette pesquisável com atalhos e contexto de disponibilidade.
- Layouts persistentes por projeto.
- Modo compacto para terminais pequenos.
- Desativação de mouse, animações e caracteres Unicode decorativos.
- Respeito a `NO_COLOR` na CLI.
- Diagnóstico de capacidades do terminal acessível pela aplicação.

## 6. Suporte por banco

### 6.1 Política de versões

Cada release suporta versões ainda mantidas oficialmente pelo fornecedor na data do lançamento. A matriz de CI inclui:

- versão mais antiga ainda suportada;
- versão LTS ou recomendada pelo fornecedor;
- versão estável mais recente.

Uma versão fora dessa matriz pode funcionar, mas a UI a identifica como não verificada. Recursos são descobertos pelo handshake e não inferidos apenas pelo número da versão.

Distribuições compatíveis por protocolo, como MariaDB e derivados de PostgreSQL, não são consideradas oficialmente suportadas até receberem driver ou matriz de conformidade próprios.

### 6.2 PostgreSQL

Além do modelo comum, o driver contempla schemas, extensions, enums, domains, sequences, materialized views, partitions, policies, tablespaces, foreign data wrappers, publications, subscriptions, functions, procedures, triggers, roles e grants. Explain estruturado, notices, cancelamento, tipos array/JSON/UUID/interval e detalhes de transação permanecem nativos do driver.

### 6.3 MySQL

Além do modelo comum, o driver contempla engines, character sets, collations, generated columns, partitions, events, routines, triggers, users, roles e grants. O driver preserva diferenças entre databases e schemas, regras de autocommit, tipos unsigned, enums/sets, JSON, zero dates quando retornadas e formatos de explain suportados pela versão.

## 7. Arquitetura

### 7.1 Estratégia escolhida

Dexo usa um Cargo workspace modular com drivers oficiais orientados a capacidades e compilados no binário. Esta opção equilibra desempenho, isolamento conceitual, segurança de tipos e distribuição simples. Não há ABI dinâmica nem RPC entre processos na primeira arquitetura.

### 7.2 Mapa de crates

| Crate | Responsabilidade |
| --- | --- |
| `dexo` | Binário, bootstrap, composição de dependências e seleção TUI/CLI |
| `dexo-app` | Casos de uso, estado, comandos, eventos e políticas de aplicação |
| `dexo-tui` | Loop visual, painéis, editor, grade, modais, temas e input |
| `dexo-cli` | Parsing de comandos, presenters, stdout/stderr e exit codes |
| `dexo-driver-api` | Contratos de driver, capacidades e tipos canônicos |
| `dexo-driver-postgres` | Protocolo, catálogo, dialeto e recursos PostgreSQL |
| `dexo-driver-mysql` | Protocolo, catálogo, dialeto e recursos MySQL |
| `dexo-sql` | Documento, parsing incremental, contexto e serviços SQL |
| `dexo-storage` | SQLite local, migrations, repositories e retenção |
| `dexo-secrets` | Keychain e referências opacas a segredos |
| `dexo-transport` | TLS, SSH tunnel, proxy e conectores de stream |
| `dexo-runtime` | Tarefas, cancelamento, canais, backpressure e progresso |
| `dexo-test-support` | Fixtures, containers, contract suites e utilitários de teste |

### 7.3 Regra de dependência

TUI e CLI dependem de `dexo-app`. A aplicação depende dos contratos, não dos drivers concretos. Drivers e infraestrutura implementam contratos e são registrados apenas em `dexo`. Nenhum driver importa código visual. `dexo-driver-api` não depende de Tokio quando um tipo de domínio síncrono for suficiente; detalhes assíncronos aparecem apenas nos contratos que realizam I/O.

### 7.4 API orientada a capacidades

A API evita um trait monolítico. Grupos conceituais incluem:

- `ConnectionFactory` e `Session`;
- `CatalogReader`;
- `QueryExecutor` e `QueryCanceller`;
- `TransactionControl`;
- `DataMutator`;
- `DdlGenerator` e `DdlExecutor`;
- `ExplainProvider`;
- `AdministrationProvider`;
- `ImportProvider` e `ExportProvider`;
- `DialectService`.

Cada sessão publica capacidades com estado `available`, `unavailable` ou `restricted`, incluindo justificativa por versão, permissão ou modo de conexão. A UI consulta capacidades; ela não compara nomes de drivers.

### 7.5 Tipos canônicos

O núcleo define identificadores qualificados, objetos de catálogo, metadados de coluna, valores tabulares, lotes de linhas, change sets, mensagens, planos genéricos e erros categorizados. Tipos específicos permanecem representáveis por:

- valor bruto sem perda;
- tipo nativo e nome qualificado;
- representação textual segura;
- metadados extras namespaced pelo driver.

Conversões potencialmente destrutivas nunca são implícitas. A grade distingue valor completo, valor truncado e valor ainda não carregado.

### 7.6 Modelo de execução

O loop da TUI recebe input, eventos de runtime e ticks de renderização. Ele atualiza estado rapidamente e agenda efeitos; nunca aguarda rede ou disco lento. Tarefas longas publicam progresso e resultados por canais limitados. Consumidor lento aplica backpressure ao produtor. Cada tarefa possui identidade, dono, estado, token de cancelamento e política de encerramento.

O runtime separa:

- tarefas interativas de baixa latência;
- consultas e introspecções;
- transferências longas;
- subprocessos externos.

Fechar uma aba não abandona silenciosamente uma consulta. O usuário escolhe cancelar, manter em background ou voltar.

## 8. Persistência local

### 8.1 SQLite

O SQLite local armazena apenas dados não secretos:

- schema version e migrations aplicadas;
- projetos, grupos e conexões sem credenciais;
- referências opacas para itens do keychain;
- documentos recuperáveis e estado de sessão;
- histórico SQL conforme política de retenção;
- snippets, favoritos e consultas nomeadas;
- layouts, temas e atalhos;
- cache e snapshots de catálogo;
- tarefas recentes e metadados de exportação.

Migrations do armazenamento criam backup recuperável antes de mudanças destrutivas. Escritas usam transações. Arquivos temporários e journals permanecem no diretório de dados da aplicação.

### 8.2 Arquivos de configuração

Configurações portáveis e adequadas à edição manual usam TOML. Paths seguem convenções nativas de cada sistema operacional. Configuração inválida aponta arquivo, campo e motivo; valores desconhecidos são preservados quando possível para compatibilidade futura.

### 8.3 Keychain

Cada conexão referencia um identificador aleatório, não o segredo. O item do keychain contém somente material secreto necessário. Renomear projeto ou conexão não muda o identificador. Remover conexão pergunta separadamente se o segredo deve ser removido do keychain.

## 9. Fluxos críticos

### 9.1 Abertura de conexão

1. Carregar metadados não sensíveis.
2. Solicitar o segredo ao keychain ou ao usuário quando ausente.
3. Validar configuração TLS, proxy e SSH.
4. Verificar host key e estabelecer tunnel quando aplicável.
5. Abrir protocolo do banco por stream abstrato.
6. Autenticar e coletar versão, contexto e capacidades.
7. Criar sessão e iniciar introspecção incremental.
8. Atualizar UI e histórico sem registrar segredo.

### 9.2 Execução de consulta

1. Determinar seleção ou statement sob o cursor.
2. Classificar operação e parâmetros.
3. Avaliar políticas de proteção da conexão.
4. Confirmar quando necessário.
5. Agendar tarefa cancelável.
6. Receber metadados e lotes com backpressure.
7. Mostrar a primeira página imediatamente.
8. Registrar resultado sanitizado conforme retenção.
9. Invalidar catálogo quando DDL for confirmado.

### 9.3 Edição de dados

1. Confirmar que existe identidade de linha confiável.
2. Acumular alterações em change set.
3. Validar tipos e constraints conhecidas localmente.
4. Mostrar preview de operações e valores.
5. Executar em transação apropriada.
6. Verificar linhas afetadas e conflitos.
7. Commit explícito ou rollback em caso de falha/política.
8. Atualizar grade e histórico.

### 9.4 Schema diff

1. Introspectar ou carregar snapshots de origem e destino.
2. Normalizar somente equivalências comprovadas pelo driver.
3. Calcular diferenças e dependências.
4. Classificar risco e reversibilidade.
5. Renderizar relatório e script.
6. Permitir edição e salvamento.
7. Aplicar apenas pelo fluxo explícito de execução protegida.

## 10. Segurança e proteção contra acidentes

### 10.1 Segredos e logs

- Segredos não entram em SQLite, TOML, argumentos de subprocesso, panic reports ou tracing spans.
- Connection strings e URLs são estruturadas e redigidas antes de logar.
- Parâmetros SQL ficam fora de logs por padrão.
- Diagnósticos exportáveis passam por sanitização e preview.
- Dumps de memória e crash reports automáticos não são enviados.

### 10.2 TLS e SSH

- Verificação TLS é o padrão.
- CA e client certificates personalizados são suportados.
- Desativar verificação exige configuração explícita e indicador permanente.
- Known hosts é verificado; chave alterada nunca é aceita automaticamente.
- Chaves privadas e passphrases usam keychain ou agent quando possível.

### 10.3 Políticas configuráveis

Políticas podem variar por conexão e ambiente. O conjunto padrão de produção é mais rígido e contempla:

- modo somente leitura;
- confirmação de `DROP`, `TRUNCATE` e alterações administrativas;
- confirmação de `UPDATE`/`DELETE` sem condição detectável;
- exigência de digitar o nome do alvo em operações de alto impacto;
- aviso para explain que executa a consulta;
- limite de linhas, duração e tamanho de exportação;
- confirmação de commit com muitas linhas afetadas.

O analisador de segurança é conservador. Falha em compreender SQL não significa que o statement seja seguro. Ao mesmo tempo, a aplicação não bloqueia extensões válidas para sempre: um usuário autorizado pode confirmar a execução conforme a política.

## 11. Erros e recuperação

Erros públicos possuem categoria, mensagem segura, causa técnica opcional, contexto, código do servidor, posição SQL quando disponível e indicação de retry. Categorias estáveis incluem:

- configuração;
- autenticação;
- rede;
- TLS/SSH;
- permissão;
- sintaxe;
- constraint/conflito;
- timeout;
- cancelamento;
- capacidade indisponível;
- armazenamento local;
- ferramenta externa;
- bug interno.

Reconexão automática é permitida somente quando não repete efeito mutável nem mascara estado transacional perdido. Uma conexão perdida durante transação é marcada como estado desconhecido e nunca reaproveitada silenciosamente.

O terminal é restaurado mesmo após erro fatal. Documentos não sensíveis e layout usam checkpoints. Na próxima abertura, o usuário recebe opções de recuperar ou descartar a sessão.

## 12. Design da TUI

### 12.1 Layout padrão

- Barra superior: projeto, conexão, database/schema e ambiente.
- Painel esquerdo: explorador, busca e favoritos.
- Centro: abas de SQL, dados, DDL, propriedades e explain.
- Painel inferior: resultados, mensagens, histórico e tarefas.
- Inspetor lateral opcional: detalhes, dependências e ações.
- Status bar: conexão, transação, tarefa, tempo, linhas e atalhos contextuais.

Painéis podem ser ocultados, alternados e redimensionados. Em terminal pequeno, o modo compacto mostra um painel por vez. O layout é salvo por projeto.

### 12.2 Modelo de interação

Eventos de teclado são interpretados por contexto e depois convertidos em comandos da aplicação. A command palette lista todas as ações disponíveis, indisponíveis e seus motivos. Modais não podem ocultar uma transação aberta ou trocar silenciosamente o contexto da conexão.

Produção usa texto/ícone persistente além de cor. Ações destrutivas mostram alvo totalmente qualificado, conexão e database antes da confirmação.

## 13. Stack aprovada

- Rust stable, edition 2024, MSRV explícita e `Cargo.lock` versionado.
- Ratatui e Crossterm para TUI multiplataforma.
- Clap derive para CLI e completions.
- Tokio e `tokio-util` para runtime, canais e cancelamento.
- `tokio-postgres` para PostgreSQL.
- `mysql_async` para MySQL.
- Rustls e certificados da plataforma para TLS.
- `russh` para SSH tunnels.
- `keyring` para cofres nativos.
- `rusqlite` com SQLite bundled para estado local.
- Ropey, Tree-sitter e gramática SQL tolerante para edição incremental.
- `sqlparser-rs` como apoio semântico, nunca como autoridade completa do dialeto.
- Serde, TOML, JSON, UUID, tipos temporais e decimais sem perda.
- `tracing` para observabilidade local sanitizada.
- `thiserror` nos crates; `anyhow` apenas na fronteira do binário.

Versões exatas são escolhidas e fixadas no primeiro plano de implementação, depois mantidas pelo `Cargo.lock`. Dependências novas exigem justificativa de capacidade, manutenção e licença.

## 14. Estratégia de testes

### 14.1 Unitários

Cobrem domínio, classificação de segurança, quoting, valores, change sets, diff, comandos, reducers e políticas. Testes não dependem de tempo real ou rede quando isso não é parte do comportamento.

### 14.2 Property tests e fuzzing

Cobrem splitting de statements, parsers tolerantes, decoders, escaping, identificadores, serialização e DDL. Invariantes incluem ausência de panic, round-trip quando definido e impossibilidade de remover quoting necessário.

### 14.3 Contract tests de driver

A mesma suíte valida conexão, catálogo, tipos, streaming, cancelamento, transações, erros e capacidades. Cada driver acrescenta suítes específicas. Testcontainers inicia versões reais de PostgreSQL e MySQL.

### 14.4 TUI e CLI

- Snapshots do backend de teste do Ratatui em dimensões e temas diferentes.
- Injeção determinística de input e eventos.
- Testes de foco, command palette, modais e restauração do terminal.
- Golden tests de stdout, stderr e exit codes.
- Testes sem TTY e com `--non-interactive`.

### 14.5 Resiliência e segurança

- Desconexão durante leitura, escrita e transação.
- Timeout, cancelamento e consumidor lento.
- Falha ou encerramento de subprocesso.
- Keychain indisponível ou bloqueado.
- Certificado inválido e host key alterada.
- Asserções de que segredos sentinela não aparecem em logs, erros, histórico ou artefatos.

### 14.6 Matriz e ferramentas

`cargo-nextest` executa a suíte em CI. GitHub Actions usa runners nativos de Linux, macOS e Windows. `rustfmt`, Clippy, `cargo-deny` e auditoria RustSec bloqueiam regressões. Testes com containers são separados dos unitários, mas obrigatórios antes de release.

## 15. Metas de desempenho

As medições usam hardware, sistema, terminal, dataset e metodologia registrados junto ao benchmark.

| Métrica | Meta |
| --- | --- |
| Primeira tela sem auto conexão | até 300 ms no p95 |
| Input até frame visível | até 50 ms no p95 |
| Busca em catálogo de 100 mil objetos | até 100 ms no p95 |
| Consulta | primeira página após o primeiro lote, sem aguardar o total |
| Exportação grande | memória incremental limitada e aproximadamente constante |
| Loop visual | nenhuma espera por rede, banco, disco lento ou subprocesso |

Caches são limitados simultaneamente por contagem e bytes. Valores grandes são carregados sob demanda. Benchmarks cobrem startup, renderização de grid, busca, parsing incremental, conversão de lotes, exportação e diff de catálogo.

## 16. Distribuição, manutenção e privacidade

- Licença dual MIT OR Apache-2.0.
- Versionamento semântico.
- Releases reproduzíveis com checksums, assinaturas e SBOM.
- `cargo-dist` gera artefatos e instaladores para Linux, macOS e Windows.
- Canais iniciais: arquivo compactado, shell installer, PowerShell installer, Homebrew e MSI quando suportado pela matriz.
- Configuração e SQLite possuem versão e migração automática.
- Mudanças destrutivas no estado local geram backup antes da migração.
- Não existe telemetria obrigatória nem envio automático de diagnóstico.
- Relatórios de diagnóstico são gerados somente por ação explícita, sanitizados e revisáveis.

## 17. Roadmap

### Marco 1 — Fundação e conexões

Workspace, qualidade, runtime, storage, keychain, transportes, drivers PostgreSQL/MySQL, gerenciador de conexões, explorador inicial e vertical slice de `SELECT` por TUI e CLI.

### Marco 2 — Workbench SQL

Documentos, parser incremental, highlighting, autocomplete, formatação, parâmetros, histórico, snippets, múltiplos resultados, transações, cancelamento e exportação de resultados.

### Marco 3 — Catálogo, dados e DDL

Introspecção completa, busca, dependências, grade editável, foreign-key navigation, formulários de objetos, DDL, usuários e permissões.

### Marco 4 — Engenharia e operações

Schema diff, scripts de migração, import/export completo, explain estruturado, sessões, locks, estatísticas, administração e integrações de backup/restore.

### Marco 5 — Paridade e versão 1.0

Matriz de versões, otimização para escala, acessibilidade, recuperação, compatibilidade de terminais, instaladores, documentação e auditoria de segurança.

Cada marco recebe uma spec técnica e um plano separado. O primeiro plano detalhado implementará a menor vertical slice que valida todas as fronteiras do Marco 1: iniciar Dexo, escolher TUI ou CLI, cadastrar uma conexão com segredo no keychain, conectar a PostgreSQL ou MySQL, executar uma consulta paginada e cancelar ou encerrar com segurança.

## 18. Definition of Done

Uma capacidade só está concluída quando:

1. Está exposta no driver aplicável e declara corretamente indisponibilidade nos demais.
2. Tem fluxo TUI e command palette; possui CLI quando o caso é automatizável.
3. Trata sucesso, erro, cancelamento e permissão insuficiente.
4. Não bloqueia o loop da TUI nem cresce memória sem limite.
5. Respeita políticas de segurança e sanitização.
6. Tem testes unitários e integração proporcional ao risco.
7. Está documentada para usuário e mantenedor.
8. Funciona na matriz oficial de plataformas e versões aplicável.

## 19. Riscos e mitigação

| Risco | Mitigação |
| --- | --- |
| Abstração comum esconder recursos nativos | Traits pequenos, capability matrix e extensões tipadas por driver |
| Parser genérico rejeitar SQL válido | Parsing tolerante; servidor permanece autoridade final |
| Resultados grandes esgotarem memória | Streaming, backpressure, paginação e limites por bytes/linhas |
| TUI congelar durante I/O | Estado/eventos e efeitos assíncronos fora do loop visual |
| Alteração atingir linhas inesperadas | Identidade confiável, change set, preview e validação de affected rows |
| Segredo aparecer em logs/subprocessos | Tipos secretos, sanitização central e testes sentinela |
| Diferenças de terminal e keychain | Matriz nativa de CI, degradação de capacidades e fallback solicitando senha |
| Escopo equivalente a IDE atrasar entregas | Marcos verticais, software utilizável ao fim de cada marco e planos separados |
| Schema diff gerar DDL perigoso | Grafo de dependências, classificação de risco e script revisável por padrão |
| Dependência de ferramentas de backup | Detecção explícita, compatibilidade de versão e mensagens acionáveis |

## 20. Referências primárias da stack

- [Ratatui — aplicação assíncrona](https://ratatui.rs/tutorials/counter-async-app/)
- [tokio-postgres](https://docs.rs/tokio-postgres/latest/tokio_postgres/)
- [mysql_async](https://docs.rs/mysql_async/latest/mysql_async/)
- [Russh](https://docs.rs/crate/russh/latest/source/README.md)
- [Keyring](https://docs.rs/keyring/latest/keyring/)
- [Rusqlite](https://docs.rs/rusqlite/latest/rusqlite/)
- [Tree-sitter SQL grammar](https://docs.rs/crate/tree-sitter-sequel/latest)
- [cargo-nextest](https://www.nexte.st/)
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/)
- [cargo-dist](https://axodotdev.github.io/cargo-dist/book/reference/config.html)
