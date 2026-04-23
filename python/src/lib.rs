mod daemon;
mod error;
mod runtime;
mod stub;
mod test_server;

use pyo3::prelude::*;

#[pymodule]
fn _rust(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<daemon::DaemonProcessHandle>()?;
    module.add_function(wrap_pyfunction!(daemon::quit_daemon, module)?)?;
    module.add_function(wrap_pyfunction!(daemon::read_user_config_json, module)?)?;
    module.add_function(wrap_pyfunction!(daemon::spawn_daemon_process, module)?)?;
    module.add_function(wrap_pyfunction!(test_server::start_test_server, module)?)?;
    module.add_class::<test_server::TestServerHandle>()?;
    Ok(())
}
