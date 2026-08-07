"""The framework-agnostic core: MCP/REST transport, rendering, and principal
handling (decision 8). LangChain and LangGraph surfaces are thin shims over
this module, and this module must never import either — see
``test_the_core_module_imports_with_no_framework_installed``.
"""
