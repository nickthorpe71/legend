import { useMemo } from 'react';
import {
  ReactFlow,
  Background,
  type Node,
  type Edge,
} from '@xyflow/react';
import { useMemoryStore } from '../stores/memoryStore';

export function L3Panel() {
  const l3 = useMemoryStore((s) => s.l3);

  const { nodes, edges } = useMemo(() => {
    if (!l3) return { nodes: [], edges: [] };

    const nodeEntries = Object.values(l3.nodes);
    const nodes: Node[] = nodeEntries.map((node, i) => {
      const cols = 5;
      const kindColor = node.kind === 'Summary' ? '#40f070' : node.kind === 'Topic' ? '#f0a030' : '#40c8e0';
      return {
        id: String(node.id),
        position: { x: (i % cols) * 200 + 20, y: Math.floor(i / cols) * 100 + 20 },
        data: { label: `${node.label}\n(${node.kind}) w:${node.weight.toFixed(1)}` },
        style: {
          background: '#0a0a0f',
          color: kindColor,
          border: `1px solid ${kindColor}40`,
          fontSize: '10px',
          fontFamily: 'inherit',
          padding: '4px 8px',
          maxWidth: '180px',
          whiteSpace: 'pre-wrap' as const,
        },
      };
    });

    const edges: Edge[] = l3.edges.map((edge, i) => ({
      id: `e${i}`,
      source: String(edge.source),
      target: String(edge.target),
      label: edge.kind,
      style: { stroke: '#2a2a34', strokeWidth: Math.max(1, edge.weight) },
      labelStyle: { fill: '#606070', fontSize: '8px' },
    }));

    return { nodes, edges };
  }, [l3]);

  if (!l3) return <div className="p-3 text-[var(--text)]">No graph data</div>;

  return (
    <div className="h-full">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        fitView
        proOptions={{ hideAttribution: true }}
        nodesDraggable
        nodesConnectable={false}
      >
        <Background color="#1a1a24" gap={40} />
      </ReactFlow>
    </div>
  );
}
