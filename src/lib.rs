pub mod logger;
pub mod models;
pub mod runtime;
pub mod server;

use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;
/// Starts the coder-mcp server with the specified workspace directory and port.
///
/// Args:
///     workspace (str): The path to the workspace directory.
///     port (int): The port number to listen on.
#[gen_stub_pyfunction]
#[pyfunction]
fn start_server(workspace: String, port: u16) -> PyResult<()> {
    let workspace_path = std::path::PathBuf::from(workspace);
    // Create a new tokio runtime to run the server
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(server::run_server(workspace_path, port));
    Ok(())
}

#[pymodule]
fn coder_mcp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(start_server, m)?)?;
    Ok(())
}
pub mod service;
pub mod tools;
