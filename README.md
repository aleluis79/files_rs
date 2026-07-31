# files_rs (ncrs)

Administrador de archivos de doble panel estilo Norton Commander, escrito en Rust con interfaz de texto (TUI).

## Características

- **Doble panel** — navegación independiente en dos directorios simultáneamente
- **Visor de texto** — visualización con scroll vertical y horizontal (`F3`)
- **Editor integrado** — edición directa en TUI (`F4`) con cursor, selección, copiar/pegar y deshacer
- **Selección múltiple** — marca archivos con `Espacio` para operar sobre varios a la vez
- **Operaciones de archivo** — copiar, mover/renombrar, crear directorio y eliminar
- **Integración con shell** — cambia el directorio de la terminal al salir de la aplicación

## Teclas

### Paneles

| Tecla | Acción |
|-------|--------|
| `↑` / `↓` | Mover cursor |
| `Enter` | Entrar en directorio / abrir archivo |
| `Tab` | Cambiar de panel activo |
| `H` | Alternar archivos ocultos |
| `Espacio` | Marcar / desmarcar archivo |
| `F3` | Ver archivo |
| `F4` | Editar archivo |
| `F5` | Copiar |
| `F6` | Mover / Renombrar |
| `F7` | Crear directorio |
| `F8` | Eliminar |
| `F9` / `Shift+F9` | Cambiar orden y dirección |
| `F12` / `Shift+F12` | Conectar / desconectar SCP |
| `F10` | Salir |
| `Esc` | Cancelar operación / cerrar visor |

### Visor/Editor (`F3`/`F4`)

| Tecla | Acción |
|-------|--------|
| `F4` | Entrar en modo edición |
| `F3` / `Esc` | Salir del visor/editor (con guardar/descartar si hay cambios) |
| `↑` / `↓` / `←` / `→` | Mover cursor en edición |
| `PageUp` / `PageDown` | Desplazamiento rápido vertical |
| `Home` / `End` | Ir al inicio/fin de línea |
| `Shift + flechas` | Seleccionar texto |
| `Ctrl+C` | Copiar selección |
| `Ctrl+V` | Pegar |
| `Ctrl+Z` | Deshacer |
| `Enter` | Nueva línea |
| `Backspace` / `Delete` | Borrar carácter o selección |

## Instalación

Requiere [Rust](https://rustup.rs/) (edición 2024).

```bash
git clone <repositorio>
cd files
cargo build --release
```

El binario queda en `target/release/files_rs`.

## Integración con shell

Para que la terminal cambie automáticamente al último directorio visitado al salir, añade esto a tu `.bashrc` o `.zshrc`:

```sh
source /ruta/a/files/shell-integration.sh
```

Luego ejecuta el programa con:

```bash
ncrs
```

## Temas

El archivo de configuración se guarda en `~/.config/files-rs/config.toml` y el valor `theme_name` ahora resuelve archivos TOML dentro de `~/.config/files-rs/themes/`.

Al iniciar la aplicación se crean, si no existen, estos temas editables:

- `~/.config/files-rs/themes/dark.toml`
- `~/.config/files-rs/themes/light.toml`
- `~/.config/files-rs/themes/solarized.toml`

Ejemplo de configuración:

```toml
theme_name = "dark"
```

También puedes crear tu propio archivo en ese directorio y referenciarlo por nombre:

```toml
theme_name = "mi-tema"
```

Eso cargará `~/.config/files-rs/themes/mi-tema.toml`. Si prefieres, también puedes usar una ruta relativa dentro del directorio de temas o una ruta absoluta a un archivo TOML.

Cada archivo de tema usa este formato:

```toml
header_fg = "yellow"
border_active = "green"
border_inactive = "darkgray"
selected_bg = "blue"
selected_fg = "white"
panel_bg = "reset"
panel_fg = "reset"
text_normal = "white"
text_dim = "gray"
text_accent = "cyan"
text_success = "green"
text_warning = "yellow"
text_error = "lightred"
gauge_fill = "green"
gauge_bg = "black"
status_fg = "cyan"
```

Los colores aceptan nombres ANSI como `blue`, `darkgray` y `lightred`, colores RGB en formato `#RRGGBB` y colores indexados en formato `ansi(123)`.

## Dependencias

| Crate | Uso |
|-------|-----|
| [ratatui](https://github.com/ratatui-org/ratatui) | Renderizado de la TUI |
| [crossterm](https://github.com/crossterm-rs/crossterm) | Entrada de teclado y ratón |
| [anyhow](https://github.com/dtolnay/anyhow) | Manejo de errores |
| [chrono](https://github.com/chronotope/chrono) | Fechas de modificación de archivos |
| [users](https://github.com/ogham/rust-users) | Información de usuario del sistema |

## Licencia

MIT
