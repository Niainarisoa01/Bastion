/* =============================================
   BASTION DASHBOARD — app.js
   WebSocket Client + SPA Router + uPlot Charts
   ============================================= */

// ── State ──
const state = {
    ws: null,
    reconnectDelay: 1000,
    maxReconnect: 30000,
    startTime: Date.now(),
    lastData: null,
    prevRequests: 0,
    reqRateHistory: new Float64Array(120).fill(0),
    latP50History: new Float64Array(120).fill(0),
    latP95History: new Float64Array(120).fill(0),
    latP99History: new Float64Array(120).fill(0),
    timeHistory: new Float64Array(120).fill(0),
    histIdx: 0,
    charts: {},
    alerts: [],
};

// Fill initial time axis
for (let i = 0; i < 120; i++) {
    state.timeHistory[i] = (Date.now() / 1000) - (120 - i);
}

// ── SPA Router ──
function initRouter() {
    document.querySelectorAll('.nav-link').forEach(link => {
        link.addEventListener('click', (e) => {
            const page = link.getAttribute('data-page');
            navigateTo(page);
        });
    });
    // Handle initial hash
    const hash = window.location.hash.replace('#', '') || 'dashboard';
    navigateTo(hash);
}

function navigateTo(page) {
    // Hide all pages
    document.querySelectorAll('.page').forEach(p => p.classList.add('hidden'));
    // Show target
    const target = document.getElementById(`page-${page}`);
    if (target) target.classList.remove('hidden');
    // Update nav active state
    document.querySelectorAll('.nav-link').forEach(l => l.classList.remove('active'));
    const activeLink = document.querySelector(`[data-page="${page}"]`);
    if (activeLink) activeLink.classList.add('active');
    // Update title
    const titles = { dashboard: 'Dashboard', backends: 'Backends', routes: 'Routes', alerts: 'Alertes' };
    document.getElementById('pageTitle').textContent = titles[page] || 'Dashboard';
}

// ── WebSocket ──
function connectWS() {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${proto}//${window.location.host}/ws/metrics`;

    state.ws = new WebSocket(wsUrl);

    state.ws.onopen = () => {
        state.reconnectDelay = 1000;
        const dot = document.querySelector('.ws-dot');
        const label = document.querySelector('.ws-label');
        dot.classList.add('connected');
        label.textContent = 'Connecté';
    };

    state.ws.onclose = () => {
        const dot = document.querySelector('.ws-dot');
        const label = document.querySelector('.ws-label');
        dot.classList.remove('connected');
        label.textContent = 'Déconnecté';
        // Reconnect with backoff
        setTimeout(() => {
            state.reconnectDelay = Math.min(state.reconnectDelay * 1.5, state.maxReconnect);
            connectWS();
        }, state.reconnectDelay);
    };

    state.ws.onerror = () => { state.ws.close(); };

    state.ws.onmessage = (event) => {
        try {
            const data = JSON.parse(event.data);
            if (data.type === 'metrics') {
                handleMetrics(data);
            }
        } catch (e) { /* skip */ }
    };
}

// ── Handle Incoming Metrics ──
function handleMetrics(data) {
    const g = data.global;
    const now = Date.now() / 1000;

    // Calculate req/s
    const reqRate = g.total_requests - state.prevRequests;
    state.prevRequests = g.total_requests;

    // Shift history arrays
    state.timeHistory.copyWithin(0, 1);
    state.timeHistory[119] = now;
    state.reqRateHistory.copyWithin(0, 1);
    state.reqRateHistory[119] = reqRate;
    state.latP50History.copyWithin(0, 1);
    state.latP50History[119] = g.p50_us;
    state.latP95History.copyWithin(0, 1);
    state.latP95History[119] = g.p95_us;
    state.latP99History.copyWithin(0, 1);
    state.latP99History[119] = g.p99_us;

    // Update KPIs
    updateKPI('kpiRequests', formatNumber(g.total_requests));
    updateKPI('kpiReqRate', `${reqRate} req/s`);
    updateKPI('kpiLatency', `${formatNumber(g.p99_us)} <small>µs</small>`);
    document.getElementById('kpiP50').textContent = formatNumber(g.p50_us);
    document.getElementById('kpiP95').textContent = formatNumber(g.p95_us);
    updateKPI('kpiErrors', formatNumber(g.total_errors));
    updateKPI('kpiActive', formatNumber(g.active_requests));

    // Error rate
    const errRate = g.total_requests > 0 ? ((g.total_errors / g.total_requests) * 100).toFixed(2) : '0.00';
    document.getElementById('kpiErrRate').textContent = `${errRate}%`;

    // Backend count
    document.getElementById('kpiBackendCount').textContent = data.backends.length;

    // KPI bars (animated)
    animateBar('kpiReqBar', Math.min(reqRate / 100, 1) * 100);
    animateBar('kpiLatBar', Math.min(g.p99_us / 5000, 1) * 100);
    animateBar('kpiErrBar', Math.min(parseFloat(errRate) / 10, 1) * 100);
    animateBar('kpiActBar', Math.min(g.active_requests / 50, 1) * 100);

    // Update charts
    updateCharts();

    // Update backends tables
    updateBackendsTable(data.backends, 'dashBackendsBody');
    updateBackendsTable(data.backends, 'backendsBody');
    document.getElementById('backendCountBadge').textContent = `${data.backends.length} actifs`;

    // Update routes table
    updateRoutesTable(data.routes);

    // Check for alerts
    checkAlerts(g, errRate);

    // Save last data
    state.lastData = data;
}

function updateKPI(id, value) {
    const el = document.getElementById(id);
    if (el) el.innerHTML = value;
}

function animateBar(id, percent) {
    const bar = document.getElementById(id);
    if (bar) bar.style.width = `${Math.max(2, percent)}%`;
}

function formatNumber(n) {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
    if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
    return n.toString();
}

// ── uPlot Charts ──
function initCharts() {
    // Request Rate chart
    const reqOpts = {
        width: 0, height: 200,
        cursor: { show: true },
        select: { show: false },
        scales: { x: { time: true }, y: { auto: true } },
        axes: [
            { stroke: '#5a6380', grid: { stroke: 'rgba(100,120,255,0.06)' }, ticks: { stroke: 'rgba(100,120,255,0.1)' }, font: '11px Inter', },
            { stroke: '#5a6380', grid: { stroke: 'rgba(100,120,255,0.06)' }, ticks: { stroke: 'rgba(100,120,255,0.1)' }, font: '11px Inter', size: 50, },
        ],
        series: [
            {},
            { label: 'req/s', stroke: '#00e676', width: 2, fill: 'rgba(0,230,118,0.08)', paths: uPlot.paths.spline(), },
        ],
    };

    const reqEl = document.getElementById('chartRequests');
    const reqData = [Array.from(state.timeHistory), Array.from(state.reqRateHistory)];
    reqOpts.width = reqEl.clientWidth;
    state.charts.requests = new uPlot(reqOpts, reqData, reqEl);

    // Latency chart
    const latOpts = {
        width: 0, height: 200,
        cursor: { show: true },
        select: { show: false },
        scales: { x: { time: true }, y: { auto: true } },
        axes: [
            { stroke: '#5a6380', grid: { stroke: 'rgba(100,120,255,0.06)' }, ticks: { stroke: 'rgba(100,120,255,0.1)' }, font: '11px Inter', },
            { stroke: '#5a6380', grid: { stroke: 'rgba(100,120,255,0.06)' }, ticks: { stroke: 'rgba(100,120,255,0.1)' }, font: '11px Inter', size: 50, },
        ],
        series: [
            {},
            { label: 'P50', stroke: '#40c4ff', width: 2, paths: uPlot.paths.spline(), },
            { label: 'P95', stroke: '#ffab40', width: 2, paths: uPlot.paths.spline(), },
            { label: 'P99', stroke: '#ff5252', width: 2, fill: 'rgba(255,82,82,0.06)', paths: uPlot.paths.spline(), },
        ],
    };

    const latEl = document.getElementById('chartLatency');
    const latData = [Array.from(state.timeHistory), Array.from(state.latP50History), Array.from(state.latP95History), Array.from(state.latP99History)];
    latOpts.width = latEl.clientWidth;
    state.charts.latency = new uPlot(latOpts, latData, latEl);

    // Handle resize
    window.addEventListener('resize', () => {
        if (state.charts.requests) state.charts.requests.setSize({ width: reqEl.clientWidth, height: 200 });
        if (state.charts.latency) state.charts.latency.setSize({ width: latEl.clientWidth, height: 200 });
    });
}

function updateCharts() {
    if (state.charts.requests) {
        state.charts.requests.setData([
            Array.from(state.timeHistory),
            Array.from(state.reqRateHistory),
        ]);
    }
    if (state.charts.latency) {
        state.charts.latency.setData([
            Array.from(state.timeHistory),
            Array.from(state.latP50History),
            Array.from(state.latP95History),
            Array.from(state.latP99History),
        ]);
    }
}

// ── Tables ──
function updateBackendsTable(backends, bodyId) {
    const tbody = document.getElementById(bodyId);
    if (!tbody) return;

    tbody.innerHTML = backends.map(b => {
        const isHealthy = b.errors === 0 || (b.requests > 0 && (b.errors / b.requests) < 0.5);
        const statusClass = isHealthy ? 'healthy' : 'unhealthy';
        const statusText = isHealthy ? 'Healthy' : 'Degraded';
        return `<tr>
            <td>${b.url}</td>
            <td><span class="status-dot ${statusClass}"></span>${statusText}</td>
            <td>${formatNumber(b.requests)}</td>
            <td>${b.errors}</td>
            <td>${formatNumber(b.p95_us)} µs</td>
        </tr>`;
    }).join('');

    if (backends.length === 0) {
        tbody.innerHTML = '<tr><td colspan="5" style="text-align:center;color:var(--text-muted);padding:30px">Aucun backend détecté</td></tr>';
    }
}

function updateRoutesTable(routes) {
    const tbody = document.getElementById('routesBody');
    if (!tbody) return;

    // Sort by requests desc
    const sorted = [...routes].sort((a, b) => b.requests - a.requests);

    tbody.innerHTML = sorted.map(r => {
        const errRate = r.requests > 0 ? ((r.errors / r.requests) * 100).toFixed(1) : '0.0';
        const errColor = parseFloat(errRate) > 5 ? 'var(--accent-red)' : 'var(--text-secondary)';
        return `<tr>
            <td>${r.path}</td>
            <td>${formatNumber(r.requests)}</td>
            <td>${r.errors}</td>
            <td>${formatNumber(r.p95_us)} µs</td>
            <td style="color:${errColor}">${errRate}%</td>
        </tr>`;
    }).join('');

    if (routes.length === 0) {
        tbody.innerHTML = '<tr><td colspan="5" style="text-align:center;color:var(--text-muted);padding:30px">Aucune route détectée</td></tr>';
    }
}

// ── Alerts ──
function checkAlerts(g, errRate) {
    if (parseFloat(errRate) > 10 && g.total_requests > 50) {
        addAlert('<span class="material-symbols-outlined" style="color:var(--accent-red)">error</span>', 'Taux d\'erreur critique', `Le taux d'erreur est de ${errRate}% (seuil: 10%)`, 'critical');
    }
    if (g.p99_us > 1_000_000) {
        addAlert('<span class="material-symbols-outlined" style="color:var(--accent-orange)">warning</span>', 'Latence P99 élevée', `P99 = ${formatNumber(g.p99_us)} µs (seuil: 1s)`, 'warning');
    }
}

function addAlert(icon, title, desc, type) {
    // Deduplicate (same title within last 60s)
    const recent = state.alerts.find(a => a.title === title && (Date.now() - a.time) < 60000);
    if (recent) return;

    state.alerts.unshift({ icon, title, desc, type, time: Date.now() });
    state.alerts = state.alerts.slice(0, 50); // keep last 50

    renderAlerts();
}

function renderAlerts() {
    const container = document.getElementById('alertsTimeline');
    if (state.alerts.length === 0) {
        container.innerHTML = '<div class="alert-empty">Aucune alerte détectée. Tout fonctionne normalement. ✅</div>';
        return;
    }

    container.innerHTML = state.alerts.map(a => `
        <div class="alert-item">
            <div class="alert-icon">${a.icon}</div>
            <div class="alert-body">
                <div class="alert-title">${a.title}</div>
                <div class="alert-desc">${a.desc}</div>
                <div class="alert-time">${new Date(a.time).toLocaleTimeString()}</div>
            </div>
        </div>
    `).join('');
}

// ── Uptime Counter ──
function updateUptime() {
    const elapsed = Math.floor((Date.now() - state.startTime) / 1000);
    const h = Math.floor(elapsed / 3600);
    const m = Math.floor((elapsed % 3600) / 60);
    const s = elapsed % 60;
    const pad = n => n.toString().padStart(2, '0');
    document.getElementById('uptime').textContent = `Uptime: ${pad(h)}:${pad(m)}:${pad(s)}`;
}

// ── Init ──
document.addEventListener('DOMContentLoaded', () => {
    initRouter();
    initCharts();
    connectWS();
    setInterval(updateUptime, 1000);
});
