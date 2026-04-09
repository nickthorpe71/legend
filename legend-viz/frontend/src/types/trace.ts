export type PipelineStep =
  // Tick pipeline
  | 'TickStart' | 'ClockIncrement' | 'Decay' | 'StabilizeLabile'
  | 'Renormalize' | 'NormalizeGraphWeights'
  | 'ChunkText' | 'EmbedText' | 'ComputeSalience' | 'ComputeEmotionalValence'
  | 'ExtractMemoryRefs' | 'SparseOrthogonalize' | 'PushWorkingMemory'
  | 'AttentionGate' | 'TryReconsolidate' | 'FindBestMatch' | 'DiversityGate'
  | 'MergeEntryHigh' | 'MergeEntryLow' | 'InsertShortTerm' | 'UpdateGraph'
  | 'UpdateTermFrequencies' | 'PruneL2' | 'PruneL3'
  | 'CpebTagging' | 'ContextSwitchDetection' | 'FlushWorkingMemory' | 'TickEnd'
  // Query pipeline
  | 'QueryStart' | 'InferQueryMode' | 'L1Scan' | 'L2TopKSimilar'
  | 'L3SummaryRetrieval' | 'PatternCompletion' | 'AssociativePriming'
  | 'HebbianReinforce' | 'AutoReinforceTop' | 'GraphLookup' | 'QueryEnd'
  // Consolidation pipeline
  | 'ConsolidateStart' | 'ReplayConsolidation' | 'ClusterGroups'
  | 'SummarizeGroup' | 'SystemsConsolidation' | 'CreateOrMergeSummaryNode'
  | 'SemanticTopicExtraction' | 'EntityLinking' | 'MarkConsolidated' | 'ConsolidateEnd';

export type TracePayload =
  | 'None'
  | { Text: string }
  | { Number: number }
  | { Embedding: number[] }
  | { Entries: TraceEntry[] }
  | { Similarities: [number, number][] }
  | { Graph: TraceGraphNode[] }
  | { KeyValue: [string, string][] };

export interface TraceEntry {
  id: number;
  text: string;
  salience: number;
  similarity: number | null;
}

export interface TraceGraphNode {
  id: number;
  label: string;
  kind: string;
  weight: number;
}

export interface TraceEvent {
  trace_id: number;
  clock: number;
  step: PipelineStep;
  elapsed_us: number;
  payload: TracePayload;
}

export type FlowType = 'tick' | 'query' | 'consolidate';

export function getFlowType(step: PipelineStep): FlowType {
  if (step.startsWith('Tick') || ['ClockIncrement', 'Decay', 'StabilizeLabile', 'Renormalize', 'NormalizeGraphWeights', 'ChunkText', 'EmbedText', 'ComputeSalience', 'ComputeEmotionalValence', 'ExtractMemoryRefs', 'SparseOrthogonalize', 'PushWorkingMemory', 'AttentionGate', 'TryReconsolidate', 'FindBestMatch', 'DiversityGate', 'MergeEntryHigh', 'MergeEntryLow', 'InsertShortTerm', 'UpdateGraph', 'UpdateTermFrequencies', 'PruneL2', 'PruneL3', 'CpebTagging', 'ContextSwitchDetection', 'FlushWorkingMemory'].includes(step)) return 'tick';
  if (step.startsWith('Query') || ['InferQueryMode', 'L1Scan', 'L2TopKSimilar', 'L3SummaryRetrieval', 'PatternCompletion', 'AssociativePriming', 'HebbianReinforce', 'AutoReinforceTop', 'GraphLookup'].includes(step)) return 'query';
  return 'consolidate';
}

/** Get the brain region color for a pipeline step */
export function getStepColor(step: PipelineStep): string {
  switch (step) {
    case 'EmbedText': case 'ChunkText': return '#f0a030'; // entorhinal - amber
    case 'ComputeSalience': return '#c89020'; // thalamus - gold
    case 'ComputeEmotionalValence': return '#d04060'; // amygdala - rose
    case 'SparseOrthogonalize': case 'DiversityGate': return '#30b0a0'; // dentate gyrus - teal
    case 'PushWorkingMemory': case 'AttentionGate': case 'FlushWorkingMemory': case 'L1Scan': return '#40c8e0'; // prefrontal - cyan
    case 'FindBestMatch': case 'MergeEntryHigh': case 'MergeEntryLow': case 'InsertShortTerm': case 'TryReconsolidate': case 'StabilizeLabile': case 'L2TopKSimilar': case 'PatternCompletion': return '#30e060'; // hippocampus - green
    case 'UpdateGraph': case 'PruneL3': case 'L3SummaryRetrieval': case 'GraphLookup': case 'AssociativePriming': case 'HebbianReinforce': case 'CpebTagging': case 'CreateOrMergeSummaryNode': case 'SystemsConsolidation': case 'SemanticTopicExtraction': case 'EntityLinking': return '#40f070'; // neocortex - bright green
    case 'AutoReinforceTop': return '#e07020'; // basal ganglia - orange
    default: return '#a0a0b0';
  }
}
