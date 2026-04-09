import { useTraceStore } from '../stores/traceStore';
import { getStepColor } from '../types/trace';
import type { TracePayload, PipelineStep } from '../types/trace';

const STEP_DESCRIPTIONS: Partial<Record<PipelineStep, string>> = {
  TickStart: 'Begin processing a new thought/tick',
  ClockIncrement: 'Advance the global brain clock',
  Decay: 'Apply time-based decay to L2 and L3 memories',
  StabilizeLabile: 'Mark recently-retrieved labile entries as stable',
  ChunkText: 'Split input text into processable chunks',
  EmbedText: 'Generate n-gram embedding vector for chunk',
  ComputeSalience: 'Score importance via keyword matching',
  ComputeEmotionalValence: 'Score emotional intensity of content',
  ExtractMemoryRefs: 'Extract structured references (IDs, paths)',
  SparseOrthogonalize: 'Push embedding away from similar L2 entries (pattern separation)',
  PushWorkingMemory: 'Add entry to L1 working memory',
  AttentionGate: 'Decide if salience is high enough to promote to L2',
  TryReconsolidate: 'Update a labile memory in-place if related',
  FindBestMatch: 'Find most similar existing L2 entry',
  DiversityGate: 'Check word overlap to prevent false merges',
  MergeEntryHigh: 'Reinforce existing entry (high similarity match)',
  MergeEntryLow: 'Blend embeddings with existing entry (moderate match)',
  InsertShortTerm: 'Create new L2 episodic memory entry',
  UpdateGraph: 'Extract entities and update L3 knowledge graph',
  UpdateTermFrequencies: 'Update entity frequency statistics',
  PruneL2: 'Remove low-value L2 entries based on salience/age/usage',
  PruneL3: 'Remove low-weight L3 graph nodes and edges',
  CpebTagging: 'Strengthen recently-active graph edges (synaptic capture)',
  ContextSwitchDetection: 'Detect topic shift by comparing tick embeddings',
  FlushWorkingMemory: 'Promote qualifying L1 entries to L2 on context switch',
  TickEnd: 'Tick processing complete',
  QueryStart: 'Begin memory retrieval query',
  InferQueryMode: 'Classify query as code, decision, or general',
  L1Scan: 'Scan working memory for relevant entries',
  L2TopKSimilar: 'Find top-k similar L2 entries by embedding',
  L3SummaryRetrieval: 'Retrieve matching L3 summary nodes',
  PatternCompletion: 'CA3 pattern completion — expand partial cues',
  AssociativePriming: 'Spread activation through graph neighbors',
  HebbianReinforce: 'Strengthen graph edges between co-activated nodes',
  AutoReinforceTop: 'Boost the top retrieved memory entry',
  GraphLookup: 'Retrieve L3 graph nodes matching query entities',
  QueryEnd: 'Query processing complete',
  ConsolidateStart: 'Begin memory consolidation (sleep replay)',
  ReplayConsolidation: 'Replay and strengthen L3 edges',
  ClusterGroups: 'Group similar L2 entries for summarization',
  SummarizeGroup: 'Generate summary label for a cluster',
  SystemsConsolidation: 'Compute centroid embedding for high-salience groups',
  CreateOrMergeSummaryNode: 'Create or merge L3 summary node',
  SemanticTopicExtraction: 'Extract recurring entities as L3 topics',
  MarkConsolidated: 'Mark L2 entries as consolidated',
  ConsolidateEnd: 'Consolidation complete',
};

function renderKeyValueTable(pairs: [string, string][]) {
  return (
    <table className="w-full text-[12px]">
      <tbody>
        {pairs.map(([key, value], i) => (
          <tr key={i} className="border-b border-[var(--border)]">
            <td className="py-1 pr-3 text-[var(--amber)] whitespace-nowrap font-medium">{key}</td>
            <td className="py-1 text-[var(--text-bright)] break-all">{value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function renderPayload(payload: TracePayload) {
  if (payload === 'None' || payload == null) return <span className="text-[var(--text)]">No data</span>;
  if (typeof payload === 'string') return <span>{payload}</span>;
  if ('Text' in payload) return <pre className="whitespace-pre-wrap break-all m-0">{payload.Text}</pre>;
  if ('Number' in payload) return <span className="text-[var(--amber)] text-lg font-medium">{payload.Number}</span>;
  if ('Embedding' in payload) return <span>[{payload.Embedding.length} dims]</span>;
  if ('Entries' in payload) return (
    <div className="flex flex-col gap-1">
      {payload.Entries.map((e) => (
        <div key={e.id} className="text-[11px]">
          <span className="text-[var(--amber)]">#{e.id}</span> {e.text.slice(0, 100)}
          {e.similarity != null && <span className="text-[var(--cyan)] ml-2">sim:{e.similarity.toFixed(3)}</span>}
        </div>
      ))}
    </div>
  );
  if ('Similarities' in payload) return (
    <div className="flex flex-col gap-1">
      {payload.Similarities.map(([id, sim], i) => (
        <div key={i} className="text-[11px]">
          <span className="text-[var(--amber)]">#{id}</span>
          <span className="text-[var(--cyan)] ml-2">{sim.toFixed(3)}</span>
        </div>
      ))}
    </div>
  );
  if ('Graph' in payload) return (
    <div className="flex flex-col gap-1">
      {payload.Graph.map((n, i) => (
        <div key={i} className="text-[11px]">
          <span className="text-[var(--green)]">{n.label}</span>
          <span className="text-[var(--text)] ml-1">({n.kind})</span>
        </div>
      ))}
    </div>
  );
  if ('KeyValue' in payload) return renderKeyValueTable(payload.KeyValue);
  return <pre className="m-0">{JSON.stringify(payload, null, 2)}</pre>;
}

export function StepDetail() {
  const event = useTraceStore((s) => s.selectedEvent);

  return (
    <div className="panel panel-amber p-3 min-h-[180px] max-h-[300px] overflow-y-auto">
      {event ? (
        <>
          <div className="flex items-center gap-3 mb-2">
            <div className="section-header m-0" style={{ color: getStepColor(event.step) }}>
              {event.step}
            </div>
            <div className="flex gap-3 text-[10px] text-[var(--text)]">
              <span>trace:{event.trace_id}</span>
              <span>seq:{event.clock}</span>
              <span>{(event.elapsed_us / 1000).toFixed(1)}ms</span>
            </div>
          </div>
          {STEP_DESCRIPTIONS[event.step] && (
            <div className="text-[11px] text-[var(--text)] mb-2 italic">
              {STEP_DESCRIPTIONS[event.step]}
            </div>
          )}
          <div className="text-[var(--text-bright)]">
            {renderPayload(event.payload)}
          </div>
        </>
      ) : (
        <div className="flex items-center justify-center h-full text-[var(--text)] text-sm">
          Select a trace event from the timeline
        </div>
      )}
    </div>
  );
}
