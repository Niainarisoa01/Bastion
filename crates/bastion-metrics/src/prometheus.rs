use crate::GatewayMetrics;
use std::sync::Arc;

pub fn export_metrics(metrics: &Arc<GatewayMetrics>) -> String {
    let mut out = String::with_capacity(1024 * 64);
    
    // Global metrics
    out.push_str(&format!("bastion_requests_total {}\n", metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed)));
    out.push_str(&format!("bastion_requests_active {}\n", metrics.active_requests.load(std::sync::atomic::Ordering::Relaxed)));
    out.push_str(&format!("bastion_errors_total {}\n", metrics.total_errors.load(std::sync::atomic::Ordering::Relaxed)));
    
    let global_hist = metrics.global_latency.lock();
    out.push_str(&format!("bastion_latency_us{{quantile=\"0.5\"}} {}\n", global_hist.value_at_quantile(0.5)));
    out.push_str(&format!("bastion_latency_us{{quantile=\"0.95\"}} {}\n", global_hist.value_at_quantile(0.95)));
    out.push_str(&format!("bastion_latency_us{{quantile=\"0.99\"}} {}\n", global_hist.value_at_quantile(0.99)));
    drop(global_hist);

    // Per route
    for entry in metrics.routes.iter() {
        let route = entry.key();
        let m = entry.value();
        out.push_str(&format!("bastion_route_requests_total{{route=\"{}\"}} {}\n", route, m.total_requests.load(std::sync::atomic::Ordering::Relaxed)));
        out.push_str(&format!("bastion_route_errors_total{{route=\"{}\"}} {}\n", route, m.total_errors.load(std::sync::atomic::Ordering::Relaxed)));
        
        let hist = m.latency.lock();
        out.push_str(&format!("bastion_route_latency_us{{route=\"{}\", quantile=\"0.5\"}} {}\n", route, hist.value_at_quantile(0.5)));
        out.push_str(&format!("bastion_route_latency_us{{route=\"{}\", quantile=\"0.95\"}} {}\n", route, hist.value_at_quantile(0.95)));
        out.push_str(&format!("bastion_route_latency_us{{route=\"{}\", quantile=\"0.99\"}} {}\n", route, hist.value_at_quantile(0.99)));
    }
    
    // Per backend
    for entry in metrics.backends.iter() {
        let backend = entry.key();
        let m = entry.value();
        out.push_str(&format!("bastion_backend_requests_total{{backend=\"{}\"}} {}\n", backend, m.total_requests.load(std::sync::atomic::Ordering::Relaxed)));
        out.push_str(&format!("bastion_backend_errors_total{{backend=\"{}\"}} {}\n", backend, m.total_errors.load(std::sync::atomic::Ordering::Relaxed)));
        
        let hist = m.latency.lock();
        out.push_str(&format!("bastion_backend_latency_us{{backend=\"{}\", quantile=\"0.5\"}} {}\n", backend, hist.value_at_quantile(0.5)));
        out.push_str(&format!("bastion_backend_latency_us{{backend=\"{}\", quantile=\"0.95\"}} {}\n", backend, hist.value_at_quantile(0.95)));
        out.push_str(&format!("bastion_backend_latency_us{{backend=\"{}\", quantile=\"0.99\"}} {}\n", backend, hist.value_at_quantile(0.99)));
    }
    
    out
}
