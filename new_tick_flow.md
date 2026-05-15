entire input operations

- extract intent
- extract primitives
    - gliner2 with all elements/relations as labels
    - also pulls out what labels actually creates results so we know which regions were activated

so now we have a list of the extracted primitives and the labels for each are effectively their perants in the DAG

IF there are no extracted primitives over a set confidence at all we can use OpenIE (need to find a pure rust lib or build one)
