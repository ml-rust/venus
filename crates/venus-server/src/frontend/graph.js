/**
 * Venus Notebook - Dependency Graph Visualization
 *
 * Uses D3.js force-directed layout to visualize cell dependencies.
 */

let graphSvg = null;
let graphSimulation = null;

/**
 * Render the dependency graph.
 *
 * @param {Map} cells - Map of cell ID to cell data
 * @param {Array} executionOrder - Array of cell IDs in execution order
 * @param {Array} edges - Optional array of dependency edges
 * @param {Array} levels - Optional array of parallel execution levels
 */
function renderGraph(cells, executionOrder, edges = null, levels = null) {
    const container = document.getElementById('graph-container');
    if (!container) return;

    // Check if D3.js is loaded
    if (typeof d3 === 'undefined') {
        container.innerHTML = '<div class="empty-state"><p>D3.js not loaded.<br>Graph visualization unavailable.</p></div>';
        return;
    }

    const width = container.clientWidth || 350;
    const height = container.clientHeight || 400;

    // Clear existing
    container.innerHTML = '';

    if (cells.size === 0) {
        container.innerHTML = '<div class="empty-state"><p>No cells to display</p></div>';
        return;
    }

    // Create SVG
    graphSvg = d3.select(container)
        .append('svg')
        .attr('width', width)
        .attr('height', height)
        .attr('viewBox', [0, 0, width, height]);

    // Add arrow marker
    graphSvg.append('defs').append('marker')
        .attr('id', 'arrowhead')
        .attr('viewBox', '-0 -5 10 10')
        .attr('refX', 30)
        .attr('refY', 0)
        .attr('orient', 'auto')
        .attr('markerWidth', 6)
        .attr('markerHeight', 6)
        .append('path')
        .attr('d', 'M0,-5L10,0L0,5')
        .attr('fill', '#30363d');

    // Build nodes
    const nodes = [];
    const nodeMap = new Map();

    executionOrder.forEach((cellId, index) => {
        const cell = cells.get(cellId);
        if (cell) {
            const node = {
                id: cellId,
                name: cell.name,
                status: cell.status,
                index: index
            };
            nodes.push(node);
            nodeMap.set(cellId, node);
        }
    });

    // Build edges from dependencies
    const links = [];

    if (edges) {
        // Use provided edges
        edges.forEach(edge => {
            if (nodeMap.has(edge.from) && nodeMap.has(edge.to)) {
                links.push({
                    source: nodeMap.get(edge.from),
                    target: nodeMap.get(edge.to),
                    param: edge.param_name
                });
            }
        });
    } else {
        // Infer edges from cell dependencies
        cells.forEach((cell, cellId) => {
            if (cell.dependencies) {
                cell.dependencies.forEach(depName => {
                    // Find the cell that produces this dependency
                    cells.forEach((otherCell, otherId) => {
                        if (otherCell.name === depName && nodeMap.has(otherId) && nodeMap.has(cellId)) {
                            links.push({
                                source: nodeMap.get(otherId),
                                target: nodeMap.get(cellId),
                                param: depName
                            });
                        }
                    });
                });
            }
        });
    }

    // Create force simulation
    graphSimulation = d3.forceSimulation(nodes)
        .force('link', d3.forceLink(links).id(d => d.id).distance(80))
        .force('charge', d3.forceManyBody().strength(-200))
        .force('center', d3.forceCenter(width / 2, height / 2))
        .force('collision', d3.forceCollide().radius(35));

    // Create container group for zoom/pan
    const g = graphSvg.append('g');

    // Add zoom behavior
    const zoom = d3.zoom()
        .scaleExtent([0.3, 3])
        .on('zoom', (event) => {
            g.attr('transform', event.transform);
        });

    graphSvg.call(zoom);

    // Draw edges
    const link = g.append('g')
        .selectAll('path')
        .data(links)
        .join('path')
        .attr('class', 'graph-edge')
        .attr('marker-end', 'url(#arrowhead)');

    // Draw nodes
    const node = g.append('g')
        .selectAll('g')
        .data(nodes)
        .join('g')
        .attr('class', d => `graph-node ${d.status}`)
        .attr('data-cell-id', d => d.id)
        .call(drag(graphSimulation));

    // Node circles
    node.append('circle')
        .attr('r', 24)
        .on('click', (event, d) => {
            // Scroll to cell
            const cellEl = document.getElementById(`cell-${d.id}`);
            if (cellEl) {
                cellEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
                cellEl.classList.add('highlight');
                setTimeout(() => cellEl.classList.remove('highlight'), 1500);
            }
        });

    // Node labels
    node.append('text')
        .text(d => truncateName(d.name, 8))
        .attr('dy', 4);

    // Tooltip
    node.append('title')
        .text(d => d.name);

    // Update positions on tick
    graphSimulation.on('tick', () => {
        link.attr('d', linkPath);

        node.attr('transform', d => {
            // Keep nodes within bounds
            d.x = Math.max(30, Math.min(width - 30, d.x));
            d.y = Math.max(30, Math.min(height - 30, d.y));
            return `translate(${d.x},${d.y})`;
        });
    });

    // Fit to view after layout settles
    graphSimulation.on('end', () => {
        fitToView(graphSvg, g, width, height, nodes);
    });
}

/**
 * Generate curved path for edges.
 */
function linkPath(d) {
    const dx = d.target.x - d.source.x;
    const dy = d.target.y - d.source.y;
    const dr = Math.sqrt(dx * dx + dy * dy) * 1.5;

    return `M${d.source.x},${d.source.y}A${dr},${dr} 0 0,1 ${d.target.x},${d.target.y}`;
}

/**
 * Drag behavior for nodes.
 */
function drag(simulation) {
    function dragstarted(event) {
        if (!event.active) simulation.alphaTarget(0.3).restart();
        event.subject.fx = event.subject.x;
        event.subject.fy = event.subject.y;
    }

    function dragged(event) {
        event.subject.fx = event.x;
        event.subject.fy = event.y;
    }

    function dragended(event) {
        if (!event.active) simulation.alphaTarget(0);
        event.subject.fx = null;
        event.subject.fy = null;
    }

    return d3.drag()
        .on('start', dragstarted)
        .on('drag', dragged)
        .on('end', dragended);
}

/**
 * Fit graph to view.
 */
function fitToView(svg, g, width, height, nodes) {
    if (nodes.length === 0) return;

    const padding = 40;
    const bounds = g.node().getBBox();

    const scale = Math.min(
        (width - 2 * padding) / bounds.width,
        (height - 2 * padding) / bounds.height,
        1.5
    );

    const tx = (width - bounds.width * scale) / 2 - bounds.x * scale;
    const ty = (height - bounds.height * scale) / 2 - bounds.y * scale;

    svg.transition()
        .duration(500)
        .call(
            d3.zoom().transform,
            d3.zoomIdentity.translate(tx, ty).scale(scale)
        );
}

/**
 * Update a node's status class.
 */
function updateGraphNodeStatus(cellId, status) {
    if (!graphSvg) return;

    const node = graphSvg.select(`[data-cell-id="${cellId}"]`);
    if (!node.empty()) {
        node.attr('class', `graph-node ${status}`);
    }
}

/**
 * Truncate cell name for display.
 */
function truncateName(name, maxLen) {
    if (name.length <= maxLen) return name;
    return name.substring(0, maxLen - 1) + '…';
}

// Export for use in app.js
window.renderGraph = renderGraph;
window.updateGraphNodeStatus = updateGraphNodeStatus;
