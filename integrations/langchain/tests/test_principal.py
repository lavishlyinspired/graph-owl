"""Slice A RED: constructing any surface without a principal must raise, and
the credential it carries must never be observable through repr, logs, or an
exception's own text — decision 2's whole point is that an integration
cannot quietly run as a superuser or leak what it authenticated with.
"""

import dataclasses

import pytest

from graph_owl_langchain._core.principal import Principal


def test_a_principal_requires_a_token():
    with pytest.raises(TypeError):
        Principal()  # type: ignore[call-arg]


def test_a_principal_rejects_an_empty_token():
    with pytest.raises(ValueError, match="token"):
        Principal(token="")


def test_a_principals_repr_never_shows_its_token():
    principal = Principal(token="sk-super-secret-value")
    assert "sk-super-secret-value" not in repr(principal)


def test_a_principals_str_never_shows_its_token():
    principal = Principal(token="sk-super-secret-value")
    assert "sk-super-secret-value" not in str(principal)


def test_a_principal_is_frozen():
    principal = Principal(token="sk-super-secret-value")
    with pytest.raises(dataclasses.FrozenInstanceError):
        principal.token = "sk-different-value"  # type: ignore[misc]
