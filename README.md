# files_rs (ncrs)

Administrador de archivos de doble panel estilo Norton Commander, escrito en Rust con interfaz de texto (TUI).

## Características

- **Doble panel** — navegación independiente en dos directorios simultáneamente
- **Visor de texto** — visualización de archivos con scroll (`F3`)
- **Selección múltiple** — marca archivos con `Espacio` para operar sobre varios a la vez
- **Operaciones de archivo** — copiar, mover/renombrar, crear directorio y eliminar
- **Integración con shell** — cambia el directorio de la terminal al salir de la aplicación

## Teclas

| Tecla | Acción |
|-------|--------|
| `↑` / `↓` | Mover cursor |
| `Enter` | Entrar en directorio / abrir archivo |
| `Tab` | Cambiar de panel activo |
| `Espacio` | Marcar / desmarcar archivo |
| `F3` | Ver archivo |
| `F5` | Copiar |
| `F6` | Mover / Renombrar |
| `F7` | Crear directorio |
| `F8` | Eliminar |
| `F10` | Salir |
| `Esc` | Cancelar operación / cerrar visor |

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
