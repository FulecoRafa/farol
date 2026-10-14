# farol — plano

Um launcher no tessela, leve antes de tudo: o oposto do Raycast. Mesmos
moldes da adwaita-shell: projeto tessela (TSX) + um binário de serviços em
Rust (`core/`, SDK `tessela-service`), commits atômicos, testes no sway
aninhado (`tessela run ../farol --nested --devtools`).

## Princípios

- **Residente e ocioso.** Nada é iniciado ao abrir; com o launcher fechado
  a superfície oculta não materializa nada (ADR 0017) e o núcleo não acorda
  (0 wakeups/s). Fontes de dados por evento, nunca por polling.
- **Busca em Rust.** O núcleo indexa e ranqueia (nucleo + níveis +
  frecência); o TSX desenha só as linhas visíveis.
- **Plugins = providers.** Um trait (`engine::Provider`): catálogo que o
  motor ranqueia, ou linhas geradas do texto; prefixo opcional. Os internos
  são compilados junto; externos falam o mesmo formato por socket.
- **Um SQLite** (`farol.db`): configurações, pseudônimos, uso, links,
  atalhos, timers, clipboard.
- **Linux completo; macOS e Windows** pela camada `platform/` (mesmas
  funções, convenções de cada SO). O tessela ainda é Unix/Wayland: portar o
  framework é uma fase própria.

## Fases

1. **Base** ✔ — núcleo, SQLite, motor, apps, calculadora (fend), emoji,
   pseudônimos e links pelo campo, superfície com teclado, preview, ações.
   Framework: `Input.onKey`, colar, apagar palavra; `Surface keyboard`.
2. **Providers** ✔ — arquivos (busca na home, recentes, preview de texto e
   imagem com miniaturas), clipboard (Wayland data-control, por evento),
   janelas (Hyprland: focar, fechar, matar, mover), timers e alarmes (com
   notificação), web (links rápidos `gh {query}`, favoritos do Helium,
   busca), plugins externos. Falta: abas do Helium (pede uma extensão no
   navegador para trocar de aba), comandos do sistema (bloquear, suspender).
3. **Atalhos** ✔ — `farol toggle [texto]` para o compositor; no Hyprland, o
   núcleo registra os atalhos do banco por `hyprctl keyword bind` (de novo
   a cada `configreloaded`) e lê `binds` para mostrar os já configurados
   e avisar conflitos. Atalhos por linha (`atalho @id`, `farol run`).
4. **Arrastar para fora** ✔ — framework: um `Box` arrastável que oferece
   `text/uri-list` ao compositor (GPUI não faz drag para fora no Wayland).
5. **macOS e Windows** — tessela: janela comum flutuante em vez de
   layer-shell, sockets por plataforma; farol: atalhos globais nativos,
   clipboard e janelas por SO. Verificação: compilação cruzada.
6. **Empacotamento** — Nix, RPM, Flatpak, Homebrew, como a adwaita-shell.

## Orçamento

| | meta |
|---|---|
| núcleo ocioso | 0 wakeups/s, < 15 MiB |
| busca por tecla | < 2 ms no núcleo |
| abrir (atalho → quadro) | < 50 ms |
