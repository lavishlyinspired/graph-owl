"""``GraphOwlRetriever`` — a LangChain ``BaseRetriever`` over graph-owl's MCP
read tools. Composes, never flattens (decision 3).

**Finding 2** (`43-framework-integrations.md`): `AssetContext.related` is FQN
strings only, with no relationship type — that lives on `explain_lineage`'s
`LineageStep` instead. So a search hit becomes one `Document` built from
*three* tool calls (`get_asset_context`, `explain_lineage`, `recall_memory`),
not one.

**A fourth finding, found writing this file**: neither `AssetContext` nor
`LineageStep` carries an Epic 6 reasoning-derived flag at all — only
`recall_memory`'s `MemoryContext.human_authored` offers anything in that
family, and it answers a related but different question (who *wrote* this,
not whether the catalog *inferred* it). This retriever reuses it for
decision 4's rendering purpose since it is the only signal Epic 14 actually
exposes; asset-context and lineage facts are rendered as asserted (never
labelled inferred) because there is nothing here to tell the difference —
defaulting to the least alarming label is the safer failure mode than
guessing "inferred" and being wrong.
"""

from __future__ import annotations

from typing import Any

from langchain_core.callbacks import CallbackManagerForRetrieverRun
from langchain_core.documents import Document
from langchain_core.retrievers import BaseRetriever
from pydantic import ConfigDict, PrivateAttr

from graph_owl_langchain._core.client import GraphOwlClient
from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain._core.rendering import (
    IGNORE_BAND_CEILING,
    GraphContext,
    RelatedFact,
    render,
    visible_facts,
)

#: Default cap on how many search hits become `Document`s. Each hit costs
#: three further tool calls, so this is a real cost knob, not a display
#: preference.
DEFAULT_LIMIT = 10


class GraphOwlRetriever(BaseRetriever):
    """Retrieves a policy-filtered subgraph as `Document`s, never flattened
    chunks (decision 3). ``principal`` has no default (decision 2)."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    endpoint: str
    principal: Principal
    max_hops: int = 2
    min_confidence: float = IGNORE_BAND_CEILING
    include_derived: bool = True
    as_of: str | None = None
    limit: int = DEFAULT_LIMIT
    #: Test-only injection point, passed straight through to `GraphOwlClient`
    #: — production code never sets this, and it exists so this class is
    #: testable without a live server, exactly as `GraphOwlClient` itself is.
    opener: Any = None

    _client: GraphOwlClient | None = PrivateAttr(default=None)

    def _get_client(self) -> GraphOwlClient:
        if self._client is None:
            self._client = GraphOwlClient(
                endpoint=self.endpoint,
                principal=self.principal,
                opener=self.opener,
            )
        return self._client

    def _get_relevant_documents(
        self, query: str, *, run_manager: CallbackManagerForRetrieverRun
    ) -> list[Document]:
        client = self._get_client()
        search = client.call_tool("search_assets", {"query": query, "limit": self.limit})
        hits = (search or {}).get("hits") or []

        documents: list[Document] = []
        for hit in hits[: self.limit]:
            fqn = hit["fullyQualifiedName"]
            context = self._fetch_context(client, fqn, query)
            facts = visible_facts(context.facts, self.include_derived, self.min_confidence)
            text, metadata = render(
                GraphContext(
                    fully_qualified_name=context.fully_qualified_name,
                    kind=context.kind,
                    description=context.description,
                    facts=facts,
                    truncated=context.truncated,
                    as_of=self.as_of,
                )
            )
            documents.append(Document(page_content=text, metadata=metadata))
        return documents

    def _fetch_context(self, client: GraphOwlClient, fqn: str, query: str) -> GraphContext:
        asset = client.call_tool("get_asset_context", {"fullyQualifiedName": fqn}) or {}
        lineage = client.call_tool("explain_lineage", {"fullyQualifiedName": fqn}) or {}
        recalled = (
            client.call_tool("recall_memory", {"fullyQualifiedName": fqn, "query": query}) or {}
        )

        facts: list[RelatedFact] = []
        for step in lineage.get("steps") or []:
            facts.append(
                RelatedFact(
                    text=f"{step['relationship']} {step['toFqn']}",
                    derived=False,
                    relationship=step.get("relationship"),
                    source=step.get("source"),
                )
            )
        for memory in recalled.get("memories") or []:
            facts.append(
                RelatedFact(
                    text=memory["content"],
                    derived=not memory.get("humanAuthored", True),
                    confidence=memory.get("confidence"),
                )
            )

        return GraphContext(
            fully_qualified_name=asset.get("fullyQualifiedName", fqn),
            kind=asset.get("kind", "unknown"),
            description=asset.get("description"),
            facts=facts,
            truncated=bool(asset.get("truncated") or lineage.get("truncated")),
        )
