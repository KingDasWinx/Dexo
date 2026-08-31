# Dexo Workspace Database-First — Design

**Status:** aprovado para planejamento

**Data:** 2026-08-31

## 1. Resumo

O Dexo deixará de tratar o editor SQL como raiz da experiência. A raiz da aplicação
passará a ser um workspace associado ao projeto ativo, composto por navigator,
abas tipadas, sessões, layout, favoritos, recentes e catálogo em cache.

O primeiro incremento arquitetural termina quando o usuário consegue executar o
fluxo abaixo sem abrir um editor SQL:

```text
abrir Dexo -> escolher projeto -> navegar em databases -> abrir uma tabela em Data
```

Edição de células, inserção, exclusão e operações em lote serão especificadas em um
incremento posterior sobre essa fundação.

## 2. Contexto e problemas atuais

O produto já possui projetos, conexões, catálogo lazy, documentos SQL, abas visuais,
persistência de layout e cache offline. Entretanto, o estado principal da TUI ainda é
SQL-first:

- o modelo inicia com foco no editor e com um `scratch.sql`;
- resultados e inspector ficam visíveis mesmo sem uma tarefa ativa;
- tabelas, propriedades e SQL compartilham estados globais em vez de estados por aba;
- conexões sem escopo claro aparecem juntas, independentemente do projeto ativo;
- a troca de projeto restaura documentos e layout, mas não troca todo o contexto do
  workspace;
- navegar até uma tabela não é tratado como um fluxo principal independente de SQL.

Isso produz muitas capacidades internas, mas uma experiência fragmentada. O novo
desenho prioriza paridade de tarefas com DataGrip e DBeaver dentro das restrições de
uma TUI, sem tentar reproduzir sua interface gráfica literalmente.

## 3. Objetivos

1. Permitir uso completo de projetos, conexões, catálogo e dados sem abrir SQL.
2. Fazer do projeto o limite explícito de persistência e organização.
3. Separar conexões exclusivas do projeto de conexões compartilhadas.
4. Restaurar o último contexto útil do usuário sem criar documentos por efeito
   colateral.
5. Permitir múltiplas abas de dados e objetos com estados independentes.
6. Mostrar catálogo em cache no primeiro frame e conectar somente sob demanda.
7. Preservar o funcionamento atual de documentos SQL durante a migração.
8. Criar limites arquiteturais adequados para a futura edição segura de dados.

## 4. Não objetivos deste incremento

- Edição de célula, inserção, clonagem ou exclusão de linhas.
- Bulk edit, bulk delete ou colagem tabular.
- Metadata editável de tabelas e escolha manual de row identity.
- Data compare, diagramas, tarefas ou agendamento.
- Novos drivers.
- Redesenho dos motores de query, schema diff, transfer, admin ou MCP.
- Paridade visual literal com interfaces desktop.

## 5. Princípios

### 5.1 Database-first, SQL optional

SQL é um tipo de documento e não o contêiner do produto. Abrir tabela, view,
propriedades, DDL ou administração não cria nem foca um console SQL.

### 5.2 Projeto como workspace

O projeto ativo determina abas, layout, favoritos, recentes, documentos, conexões
associadas e catálogo em cache. Recursos compartilhados aparecem explicitamente em
uma seção separada.

### 5.3 Cache-first, network-later

O primeiro frame usa somente estado local. A rede é acionada quando o usuário pede
dados atuais, expande conteúdo não armazenado ou solicita refresh.

### 5.4 Ações de domínio

Teclado, mouse, palette e navigator emitem as mesmas ações de domínio. Um clique na
sidebar não manipula diretamente telas ou cria documentos SQL.

### 5.5 Falhas isoladas

Falhar ao carregar um nó, aba ou conexão não invalida o restante do workspace. A
troca de projeto é atômica e preserva o projeto anterior em caso de erro.

## 6. Arquitetura do workspace

O estado raiz passa a representar explicitamente o workspace ativo:

```text
Workspace
|- ActiveProject
|- NavigatorState
|- WorkspaceTabs
|- SessionSet
|- WorkspaceLayout
|- Favorites
|- RecentItems
`- CatalogCacheContext
```

Cada unidade possui responsabilidade única:

- `ActiveProject`: identidade e metadados do projeto ativo.
- `NavigatorState`: árvore visível, seleção, expansão, filtros e estados de nós.
- `WorkspaceTabs`: ordem, aba ativa e estado independente de cada conteúdo.
- `SessionSet`: sessões pertencentes ao workspace e suas transações.
- `WorkspaceLayout`: dimensões, visibilidade e foco dos painéis.
- `Favorites` e `RecentItems`: atalhos persistidos no escopo do projeto.
- `CatalogCacheContext`: catálogo local por projeto e conexão.

Adapters TUI continuam sobre `dexo-app`. Persistência permanece em `dexo-storage` e
SQLite. Operações específicas dos bancos continuam atrás dos contratos de driver.

## 7. Escopo das conexões

Cada perfil de conexão terá um escopo explícito:

- `Project(project_id)`: aparece somente no projeto associado.
- `Shared`: aparece em uma seção separada e pode ser usado em qualquer projeto.

Uma conexão nunca deve aparecer em outro projeto por ausência de filtro. Associar,
desassociar, compartilhar ou mover uma conexão é uma ação explícita.

A sidebar do projeto será estruturada assim:

```text
Projeto: Ecommerce
|- Databases
|  |- PostgreSQL Local       conectado
|  |  `- ecommerce
|  |     `- public
|  |        |- Tables
|  |        |- Views
|  |        |- Functions
|  |        `- Sequences
|  `- MySQL Producao         desconectado [PROD]
|- Shared Databases
|  `- Analytics
|- Favorites
`- Recent
```

## 8. Inicialização e troca de projeto

### 8.1 Inicialização

1. Carregar configurações e o último projeto ativo.
2. Restaurar sidebar, abas, layout e último foco daquele projeto.
3. Se não houver abas restauráveis, criar apenas uma aba `StartCenter`.
4. Carregar o catálogo persistido sem esperar pela rede.
5. Conectar somente quando uma ação online exigir uma sessão.

Nenhum `scratch.sql` será criado automaticamente.

### 8.2 Troca de projeto

1. Bloquear a troca se houver transação aberta até commit ou rollback.
2. Solicitar decisão para documentos sujos ou mudanças de dados pendentes.
3. Persistir documentos, abas, navigator e layout do projeto atual.
4. Fechar suas sessões com segurança.
5. Carregar atomicamente o workspace do projeto destino.
6. Somente então publicar o novo projeto como ativo.

Qualquer falha mantém o workspace anterior utilizável e intacto.

## 9. Navigator

O navigator é uma superfície principal e persistente. Cada nó possui:

- identidade estável;
- tipo do objeto;
- capacidade de expansão e abertura;
- estado de conexão ou carregamento;
- origem cacheada ou online;
- ações contextuais disponíveis.

### 9.1 Interações

- `Up`/`Down`: mover a seleção.
- `Right` ou `l`: expandir.
- `Left` ou `h`: recolher ou subir ao pai.
- `Enter`: abrir o conteúdo natural.
- `Space`: participar de uma seleção múltipla quando suportado.
- `/`: buscar na árvore atual.
- `Ctrl+P`: buscar projetos, databases, objetos, abas e comandos.
- Clique: selecionar.
- Duplo clique: abrir.
- Clique na seta: expandir ou recolher.

### 9.2 Ação natural por tipo

| Tipo | Ação de `Enter` |
| --- | --- |
| Projeto | Ativar o workspace |
| Database desconectado | Conectar e expandir |
| Database conectado | Expandir ou recolher |
| Catalog ou schema | Expandir ou recolher |
| Tabela ou view | Abrir `TableData` |
| Função ou procedure | Abrir `ObjectProperties` |
| Índice, constraint ou trigger | Abrir `ObjectProperties` |
| Favorito ou recente | Abrir seu objeto original |

Abrir SQL console é sempre uma ação explícita por atalho, palette ou action sheet.

### 9.3 Estados visíveis

Os estados não dependem somente de cor:

```text
connected | disconnected | connecting | error | read-only | production | stale
```

O tema pode acrescentar cor, mas cada estado possui ícone ou texto distinguível em
modo sem cor e fallback ASCII.

### 9.4 Carregamento e cache

1. Mostrar imediatamente os nós armazenados.
2. Usar cache ao expandir conteúdo já conhecido.
3. Carregar somente o subtree solicitado quando faltar conteúdo.
4. Permitir refresh de nó, subtree, schema ou database.
5. Preservar a árvore offline e mostrar a idade do cache após falha de rede.

Expansão, filtros e seleção são persistidos por projeto e database. Filtros cobrem
nome, schema, tipo, objetos de sistema e favoritos.

### 9.5 Ações da sidebar

A barra da sidebar contém apenas as ações frequentes: adicionar database, buscar,
atualizar e abrir ações. Editar, duplicar, mover, compartilhar, desconectar, gerar
SQL, exportar, comparar e administrar ficam na action sheet pesquisável.

As entradas emitem ações como:

```text
ActivateProject
ConnectDataSource
ExpandNavigatorNode
RefreshNavigatorNode
OpenTableData
OpenObject
OpenSqlConsole
```

## 10. Abas tipadas

O workspace utiliza um enum persistível e versionado:

```text
WorkspaceTab
|- StartCenter
|- TableData
|- ObjectProperties
|- DdlViewer
|- SqlDocument
|- SchemaDiff
|- DataTransfer
|- Explain
`- Administration
```

Cada variante guarda somente seu próprio estado:

- `TableData`: conexão, tabela, página, filtro, ordenação, seleção e mudanças
  pendentes.
- `ObjectProperties`: objeto, seção interna e metadata carregada.
- `SqlDocument`: arquivo ou scratch, conexão associada, texto, cursor e execução.
- `SchemaDiff`: origens, filtros e resultado.
- `DataTransfer`: origem, destino, formato e progresso.
- `Explain` e `Administration`: alvo, visualização e estado específico.

### 10.1 Regras de ciclo de vida

- Abrir o mesmo objeto foca sua aba existente por padrão.
- `Open in New Tab` cria outra instância quando solicitado.
- Fechar aba com mudanças pendentes exige aplicar, descartar ou cancelar.
- Uma aba fica offline/stale quando sua conexão cai, sem perder contexto local.
- Reconectar preserva filtro, seleção e mudanças pendentes.
- Fechar um console não afeta abas de dados da mesma conexão.
- Fechar a conexão não fecha automaticamente suas abas.

### 10.2 Fluxo de abertura de tabela

```text
Navigator
  -> OpenTableData(connection, object)
  -> localizar ou criar TableDataTab
  -> mostrar cache ou loading
  -> garantir uma sessao
  -> carregar metadata + primeira pagina
  -> renderizar o grid
```

Metadata editável completa será adicionada no incremento de edição segura, mas a aba
já terá uma fronteira própria para recebê-la.

## 11. Persistência

O schema local ganhará estruturas equivalentes a:

- `workspace_state`: projeto ativo, aba ativa, foco e versão.
- `workspace_tabs`: projeto, ordem, tipo, título, contexto serializado e versão.
- estado do navigator: expansão, filtros e seleção por projeto/conexão.
- escopo explícito das conexões.

Não serão persistidos:

- senhas ou credenciais;
- resultados completos;
- transações;
- queries ou operações em andamento;
- handles de sessão ou recursos do runtime.

Mudanças de dados pendentes poderão ganhar recuperação em um incremento posterior,
mas nunca serão reaplicadas automaticamente.

## 12. Migração

- Conexões com `project_id` continuam associadas ao projeto existente.
- Conexões sem `project_id` tornam-se `Shared` para não desaparecerem.
- O projeto `Default` atual continua válido.
- Documentos existentes tornam-se abas `SqlDocument`.
- Layouts existentes são convertidos para o novo estado do workspace.
- Catálogos cacheados continuam utilizáveis com nova chave de escopo quando
  necessário.
- A migração do SQLite cria backup antes de alterar o schema.

Versões de aba desconhecidas ou futuras são ignoradas com diagnóstico sanitizado;
elas não causam `panic` nem impedem a abertura do projeto.

## 13. Tratamento de falhas

- Falhar ao restaurar uma aba afeta somente aquela aba.
- Falhar ao carregar um subtree afeta somente aquele nó.
- Falhar ao conectar mantém catálogo e propriedades em modo offline.
- Falhar durante troca de projeto preserva o workspace anterior.
- Transação aberta impede troca silenciosa de projeto ou sessão.
- Documentos sujos e futuras mudanças de dados pendentes exigem decisão explícita.
- Reconexão nunca repete query mutável, DDL ou mutação automaticamente.
- Erros de persistência permanecem visíveis e não são apresentados como sucesso.

## 14. Desempenho

- O primeiro frame não espera rede.
- A árvore carrega sob demanda.
- Estado persistido não contém páginas completas de resultados.
- Abas inativas podem liberar dados pesados, preservando contexto e posição.
- Busca global consulta primeiro o índice/cache local e pode enriquecer resultados em
  background.
- O custo do primeiro frame não cresce linearmente com a quantidade total de objetos
  remotos.

## 15. Verificação

### 15.1 Persistência e migração

- Migrar uma base local existente com conexões associadas e sem associação.
- Verificar backup antes da migração.
- Restaurar projeto, abas, aba ativa, foco, layout e navigator.
- Converter documentos atuais em `SqlDocument` sem perda.

### 15.2 Projetos e conexões

- Separar conexões do projeto e compartilhadas.
- Impedir vazamento de conexão exclusiva para outro projeto.
- Trocar projeto com documentos limpos.
- Bloquear troca com transação aberta.
- Confirmar troca com documentos sujos.
- Preservar o projeto anterior em falha de armazenamento ou fechamento de sessão.

### 15.3 Inicialização e abas

- Iniciar sem criar ou focar SQL.
- Abrir `StartCenter` quando não houver abas.
- Restaurar a última aba útil quando houver estado salvo.
- Abrir tabela diretamente em `TableData`.
- Abrir dois grids com filtros e páginas independentes.
- Reutilizar aba do mesmo objeto e respeitar `Open in New Tab`.

### 15.4 Navigator

- Carregar catálogo offline no primeiro frame.
- Expandir lazy e atualizar somente o subtree solicitado.
- Preservar árvore e indicar stale após falha de rede.
- Verificar paridade entre teclado, mouse e command palette.
- Verificar filtros, favoritos, recentes e objetos de sistema.

### 15.5 Qualidade e performance

- Snapshots em layouts amplo, reduzido e compacto.
- Fallback sem cor e ASCII.
- Benchmark do primeiro frame com árvore grande.
- Teste de memória para múltiplas abas inativas.
- `cargo fmt --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace --all-targets`.
- Verificações específicas de migração e release.

## 16. Critérios de aceitação

O incremento está concluído quando:

1. O Dexo inicia em `StartCenter` ou restaura uma aba existente sem criar SQL.
2. A sidebar separa databases do projeto e compartilhados.
3. Trocar projeto substitui atomicamente todo o workspace relevante.
4. O catálogo cacheado aparece sem conexão e atualiza lazy.
5. `Enter` em uma tabela abre uma aba `TableData` independente.
6. Duas abas de dados preservam filtros, páginas e seleção sem interferência.
7. SQL console só é criado por ação explícita.
8. Estado existente migra sem perda de documentos ou conexões.
9. Falhas de nó, aba ou conexão permanecem isoladas e recuperáveis.
10. A suíte de verificação definida nesta especificação passa.

## 17. Sequência posterior aprovada

Após este incremento, a evolução seguirá por especificações independentes:

1. Metadata editável de tabela e row identity segura.
2. Edição individual, insert, clone e delete.
3. Bulk edit, bulk delete e colagem tabular.
4. Explorer e object editor avançados.
5. Data grid avançado.
6. Data compare, diagramas e tarefas.

Tabelas sem chave serão somente leitura por padrão. No incremento de edição, o
usuário poderá escolher manualmente colunas identificadoras, e cada `UPDATE` ou
`DELETE` continuará obrigado a afetar exatamente uma linha.
