pub mod logger;
pub mod models;
pub mod runtime;
pub mod server;

use pyo3::prelude::*;

use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone)]
#[pyclass]
struct CServer {
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

#[pymethods]
impl CServer {
    #[new]
    fn new() -> Self {
        CServer {
            shutdown_tx: Arc::new(Mutex::new(None)),
            server_handle: Arc::new(Mutex::new(None)),
        }
    }

    fn start<'p>(
        &self,
        py: Python<'p>,
        workspace: String,
        port: u16,
    ) -> PyResult<Bound<'p, PyAny>> {
        let shutdown_tx = self.shutdown_tx.clone();
        let server_handle = self.server_handle.clone();
        let workspace_path = std::path::PathBuf::from(workspace);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (tx, rx) = oneshot::channel();
            *shutdown_tx.lock().unwrap() = Some(tx);

            // Create a new runtime for the server or rely on pyo3-asyncio's initialized runtime
            // Usually pyo3-asyncio expects a runtime to be running.
            // We spawn the server task.
            let handle = tokio::spawn(async move {
                server::run_server(workspace_path, port, rx).await;
            });

            *server_handle.lock().unwrap() = Some(handle);
            Ok(())
        })
    }

    fn stop<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let shutdown_tx_mutex = self.shutdown_tx.clone();
        let server_handle_mutex = self.server_handle.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = shutdown_tx_mutex.lock().unwrap().take();
            if let Some(tx) = tx {
                let _ = tx.send(());
            }

            let handle = server_handle_mutex.lock().unwrap().take();
            if let Some(handle) = handle {
                let _ = handle.await;
            }
            Ok(())
        })
    }
}

#[pymodule]
fn coder_mcp(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<CServer>()?;
    Ok(())
}
pub mod service;
pub mod tools;
