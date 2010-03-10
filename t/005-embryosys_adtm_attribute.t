#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(embryosys_adtm_attribute, "Module 'embryosys_adtm_attribute' loaded."),
    etap_can:has_attrib(embryosys_adtm_attribute, behavior),
    etap_can:is_attrib(embryosys_adtm_attribute, behavior, gen_server),
    etap_can:can_ok(embryosys_adtm_attribute, create),
    etap_can:can_ok(embryosys_adtm_attribute, create, 2),
    etap_can:can_ok(embryosys_adtm_attribute, create, 3),
    etap_can:can_ok(embryosys_adtm_attribute, hibern),
    etap_can:can_ok(embryosys_adtm_attribute, hibern, 2),
    etap_can:can_ok(embryosys_adtm_attribute, awake),
    etap_can:can_ok(embryosys_adtm_attribute, awake, 2),
    etap_can:can_ok(embryosys_adtm_attribute, destroy),
    etap_can:can_ok(embryosys_adtm_attribute, destroy, 2),
    etap_can:can_ok(embryosys_adtm_attribute, resur),
    etap_can:can_ok(embryosys_adtm_attribute, resur, 2),
    etap_can:can_ok(embryosys_adtm_attribute, purge),
    etap_can:can_ok(embryosys_adtm_attribute, purge, 2),

    application:start(embryosys),
    embryosys_storage_server:init_storage(),

    % First create a class
    embryosys_adtm_class:create("Bridge"),

    % Try to create an attribute
    etap:is(embryosys_adtm_attribute:create("Bridge", "height"), {ok, {adt, {meta, attribute, ["Bridge", "height"], alive},[]}}, "Creating Attribute Bridge.height"),

    % Changing the adt state
    etap:is(embryosys_adtm_attribute:hibern("Bridge", "height"), {ok, {adt, {meta, attribute, ["Bridge", "height"], frozen},[]}}, "Freezing Attribute Bridge.height"),
    etap:is(embryosys_adtm_attribute:awake("Bridge", "height"), {ok, {adt, {meta, attribute, ["Bridge", "height"], alive},[]}}, "Unfreezing Attribute Bridge.height"),
    etap:is(embryosys_adtm_attribute:destroy("Bridge", "height"), {ok, {adt, {meta, attribute, ["Bridge", "height"], destroyed},[]}}, "Destroying Attribute Bridge.height"),
    etap:is(embryosys_adtm_attribute:resur("Bridge", "height"), {ok, {adt, {meta, attribute, ["Bridge", "height"], alive},[]}}, "Resurecting Attribute Bridge.height"),

    % Try to create an attribute in a class that doesnt exist
    etap:is(embryosys_adtm_attribute:create("Nope", "height"), {wrong_state, []}, "Can't create Nope.height"),

    % Try to recreate the same attribute in the same class
    etap:is(embryosys_adtm_attribute:create("Bridge", "height"), {wrong_state, []}, "Can't create the same attribute twice"),

    application:stop(embryosys),

    etap:end_tests(),
    ok.

