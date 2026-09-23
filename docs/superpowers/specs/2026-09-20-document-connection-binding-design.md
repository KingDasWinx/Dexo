# Vínculo entre documento e conexão — Design

**Status:** implementado

**Data:** 2026-09-20

## 1. Resumo

Um documento passa a pertencer a uma conexão. A aba lembra onde executa, trocar de aba
troca a conexão ativa, e a barra de abas diz de quem é cada documento.

Hoje a barra mostra `console.sql × console.sql × console.sql × scratch.sql ×
query-4.sql ×` e não há como saber qual é qual, porque não há o que saber: a maioria
dos documentos não pertence a conexão nenhuma.

## 2. Estado atual

Três achados na leitura do código, nesta ordem de importância:

**A execução é global.** `start_query` (`update.rs:4239`) e `start_derived_script`
(`update.rs:4983`) leem `model.active_session`. Um documento roda no que estiver ativo
no instante do Enter. Não existe vínculo a ser exibido.

**O campo do vínculo existe pela metade.** `EditorDocument.connection_id`
(`model.rs:1248`) guarda o UUID do profile, não o nome, e é preenchido em dois lugares
apenas: `Action::ConnectionSqlReady` para o console, e `document.new` quando o nome vem
do prompt. Documento de tabela, arquivo aberto do disco e restaurado do banco nascem
todos sem vínculo.

**O campo da sessão está morto.** `EditorDocument.session: Option<SessionId>`
(`model.rs:1251`) nunca é escrito. É intenção não terminada, do mesmo tipo do
`ExplorerAction` que foi removido em `c1c1279`.

Há ainda um vínculo de mão única já implementado: conectar troca o documento ativo
para o console daquela conexão (`update.rs:138`). O que falta é o sentido inverso.

## 3. Objetivos

1. Todo documento tem uma conexão conhecida, ou declaradamente nenhuma.
2. Executar um documento executa na conexão dele, nunca em outra.
3. Trocar de aba troca o contexto inteiro: sessão, catálogo, status, transação.
4. A barra de abas distingue documentos de conexões diferentes sem o usuário focar
   cada um.
5. O vínculo sobrevive a um relançamento.

## 4. Não-objetivos

- Executar um mesmo documento em duas conexões (comparação lado a lado).
- Reatribuir a conexão de um documento pela UI. Fica para um incremento posterior;
  neste, o vínculo nasce com o documento e não muda.
- Agrupar visualmente as abas por conexão na barra.
- Qualquer mudança no modelo de projetos.

## 5. Decisões tomadas

Registradas porque mudam o desenho e já foram acordadas:

| Questão | Decisão |
|---|---|
| Trocar de aba muda a conexão ativa? | Sim. A aba é a fonte da verdade; sidebar, status e transação acompanham. |
| Executar com a conexão vinculada offline? | Reconecta sozinho e executa quando a sessão abrir. |
| Documento novo nasce vinculado a quê? | À conexão ativa no momento. Sem conexão ativa, nasce sem vínculo. |
| Documento sem vínculo? | Estado válido. Vincula-se à conexão ativa na primeira execução. |

A terceira alternativa considerada e descartada para o caso offline foi executar na
conexão ativa como fallback. Redirecionar uma query em silêncio para outro banco é
risco de dado, não conveniência.

## 6. O vínculo

`EditorDocument.connection_id: Option<String>` é promovido de "só o console preenche"
para o vínculo de todo documento. Continua sendo o UUID do profile.

`EditorDocument.session` é removido. Sessão é transitória: o `SessionRegistry` já mapeia
nome de conexão para sessão viva, e uma cópia no documento envelheceria no instante em
que a conexão cai e reabre com outro `SessionId`. O documento aponta para o profile; a
sessão é resolvida no uso.

Quem escreve o vínculo:

| Origem do documento | Vínculo |
|---|---|
| `EditorDocument::new_unique` via `document.new` | conexão ativa, ou `None` |
| Console de uma conexão (`ConnectionSqlReady`) | aquela conexão — já é o caso hoje |
| Documento de tabela (`open_object_data`) | a conexão de onde a tabela foi aberta |
| Arquivo aberto do disco | conexão ativa, ou `None` |
| Restaurado do banco | o que estiver persistido |
| Sem vínculo, ao executar | conexão ativa no momento da execução |

`active_connection_uuid` (`update.rs:4145`) já resolve nome para UUID e é o helper
reusado em todos esses pontos.

## 7. Troca de aba

`activate_document` (`update.rs:4777`) é o ponto único por onde toda troca de documento
passa — a garantia vem do wrapper de `update` documentado em `update.rs:14`, que existe
justamente porque `active_document` é atribuído em dezoito lugares.

Passa a fazer, além do que já faz para documentos de tabela:

1. Se o documento não tem vínculo, nada muda.
2. Se o vínculo aponta para o profile já ativo, nada muda.
3. Se aponta para outro profile **com** sessão viva, roteia por
   `activate_existing_session` (`update.rs:3586`) — que já seta `connection.*`,
   `active_session`, `session_generation`, `transaction`, re-seleciona o nó do explorer
   e carrega o catálogo.
4. Se aponta para outro profile **sem** sessão viva, emite `Effect::ConnectProfile`. O
   connect é spawned com timeout de 10 s desde `202c0c1`, então não bloqueia o loop de
   eventos, e `Action::ConnectionChanged` fecha a troca.

### 7.1 O laço

`ConnectionSqlReady` move o documento ativo para o console da conexão
(`update.rs:138`); `activate_document` passa a mover a conexão ativa para a do
documento. O ciclo termina no early-return de `activate_existing_session`
(`update.rs:3592`), que sai quando `active_session` já é a sessão pedida, e no passo 2
acima.

Isso é uma propriedade, não uma coincidência: se qualquer um dos dois passos deixar de
ser idempotente, o resultado é um loop de troca de aba que congela a TUI. Precisa de
teste dedicado (§11).

## 8. Execução

Depois de §7 a sessão ativa é a do documento, então `start_query` e
`start_derived_script` continuam corretos sem alteração.

Resta o caso em que a conexão vinculada não está conectada: o usuário pressiona Enter
numa aba cuja conexão caiu, ou que foi restaurada de um relançamento.

`ExecuteStatement`, `ExecuteSelection` e `ExecuteDocument` ganham uma pré-condição
comum: se o documento tem vínculo e ele não corresponde à sessão ativa, a execução é
enfileirada e os efeitos de conexão são emitidos primeiro.

O estado novo é um só:

```rust
/// Execução esperando a conexão do documento abrir. O token é o mesmo `connect_token`
/// que já descarta um `ConnectionChanged` obsoleto, para um connect antigo não
/// disparar a query de outra aba.
pub struct PendingExecute {
    /// Id do documento que pediu, conferido na hora de drenar: a aba pode ter mudado.
    pub document: String,
    /// Qual das três: `ExecuteStatement`, `ExecuteSelection` ou `ExecuteDocument`.
    pub action: Action,
    pub token: u64,
}
```

`ConnectionChanged` drena a fila quando o token bate e o documento ainda está ativo. Um
connect que falha emite `ConnectionFormError`, que desde `202c0c1` chega ao toast quando
o formulário está fechado; a fila é descartada nesse caminho e a query não roda.

## 9. Persistência

Sem persistir, a barra volta a mentir a cada relançamento.

`MIGRATION_13` adiciona `connection_id TEXT` a `documents`, exatamente como a 12
adicionou `kind` em `d1bb43f`. `StoredDocument`, `FlushedDocument`,
`DocumentRepository::save` e `document_from_stored` carregam o campo pelo mesmo caminho
que já carregam o `kind`.

Um documento restaurado cujo profile não existe mais — conexão deletada entre sessões —
volta sem vínculo, não com um UUID órfão.

## 10. Exibição

O título do documento passa a carregar a conexão.

**Console.** Deixa de se chamar `console.sql`, que é o nome do arquivo em disco e é
igual para todos, e passa a se chamar pelo nome da conexão. É literalmente o que ele é:
o console daquela conexão.

**Demais documentos com vínculo.** Prefixo `conexão·`, com o nome da conexão truncado em
8 células.

**Documentos sem vínculo.** Só o título, sem prefixo. A ausência é informação: esse
documento ainda não pertence a lugar nenhum.

```text
 Teste ×  WinxBuy ×  db2 ×  db2·scratch.sql ×  db2·query-4.sql ×  +
```

`MAX_TITLE_WIDTH` (`widgets/document_tabs.rs:15`) continua 20 e passa a contar o prefixo.
O truncamento existente (`truncate_cell`) corta o título, nunca o prefixo: saber de quem
é a aba importa mais do que ler o fim do nome do arquivo.

Custo medido em 80 colunas: três abas antes de rolar, contra quatro hoje. A barra já
tem scroll com indicadores `‹` e `›` e tecla dedicada, então a perda é de densidade, não
de alcance.

Não se usa cor para distinguir conexões: falha em monocromático e o `NO_COLOR` é
contrato do projeto.

## 11. Testes

Por camada, do mais barato ao mais caro:

**Transições de estado.**
- Documento novo nasce com a conexão ativa; sem conexão ativa, nasce sem vínculo.
- Trocar para uma aba de outra conexão com sessão viva ativa aquela sessão.
- Trocar para uma aba cuja conexão está offline emite `ConnectProfile`.
- Trocar para uma aba da mesma conexão não emite efeito nenhum.
- Documento sem vínculo se vincula na primeira execução.
- Executar com a conexão offline enfileira e não dispara script.
- `ConnectionChanged` com token obsoleto não drena a fila.
- Um connect que falha descarta a fila.

**O laço de §7.1.** Conectar leva ao console daquela conexão, que leva à conexão do
console, que precisa parar ali. O teste conta efeitos e falha se a sequência não
estabilizar num número fixo.

**Persistência.** Round-trip do `connection_id` por `flush_documents` e bootstrap.
Documento cujo profile sumiu volta sem vínculo.

**Frames fixados.** A barra de abas em 160, 100 e 80 colunas, com e sem vínculo, e com
o nome de conexão longo o bastante para truncar.

## 12. Riscos

| Risco | Mitigação |
|---|---|
| Loop de troca de aba congelando a TUI | Idempotência nos dois sentidos, com teste dedicado (§11) |
| Trocar de aba dispara carga de catálogo e a sidebar pula | Consequência aceita da decisão em §5; `activate_existing_session` já é o caminho que o usuário aciona à mão |
| Query disparada na conexão errada por connect obsoleto | `connect_token` na fila de execução (§8) |
| Barra de abas mais apertada | Scroll já existe; truncar o título, nunca o prefixo |
| Migration 13 e binário antigo | Mesma exposição da 12: abrir com binário anterior arquiva o banco (`database.rs:144`). Já é o comportamento conhecido do projeto |

## 13. Ordem de implementação

Os seis passos foram entregues. `Switch` (§8) ficou um enum de três casos em vez do
booleano previsto: discar outra conexão deixa a sessão anterior viva, então "ainda não
há sessão" não é o teste de quando esperar — colapsar os casos num `Option` mandou a
query para a conexão errada até o teste pegar.

Cada passo compila e passa a suíte sozinho:

1. Remover `EditorDocument.session`.
2. Preencher `connection_id` em todas as origens de documento (§6), sem consumir ainda.
3. Migration 13 e persistência do campo (§9).
4. Exibição na barra de abas (§10). A partir daqui o problema relatado está resolvido.
5. Troca de aba dirigindo a conexão (§7), com o teste do laço.
6. Fila de execução para conexão offline (§8).

Os passos 1 a 4 são entregáveis por si. Os passos 5 e 6 são os que mudam roteamento e
carregam o risco da tabela acima.
