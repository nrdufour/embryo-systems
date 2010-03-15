#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(embryosys_adtm_object, "Module 'embryosys_adtm_object' loaded."),
    etap_can:has_attrib(embryosys_adtm_object, behavior),
    etap_can:is_attrib(embryosys_adtm_object, behavior, gen_server),
    etap_can:can_ok(embryosys_adtm_object, create),
    etap_can:can_ok(embryosys_adtm_object, create, 2),
    etap_can:can_ok(embryosys_adtm_object, hibern),
    etap_can:can_ok(embryosys_adtm_object, hibern, 2),
    etap_can:can_ok(embryosys_adtm_object, awake),
    etap_can:can_ok(embryosys_adtm_object, awake, 2),
    etap_can:can_ok(embryosys_adtm_object, destroy),
    etap_can:can_ok(embryosys_adtm_object, destroy, 2),
    etap_can:can_ok(embryosys_adtm_object, resur),
    etap_can:can_ok(embryosys_adtm_object, resur, 2),
    etap_can:can_ok(embryosys_adtm_object, purge),
    etap_can:can_ok(embryosys_adtm_object, purge, 2),

    application:start(embryosys),
    embryosys_storage_server:init_storage(),

    % First create a class
    embryosys_adtm_class:create("Bridge"),

    % Try to create an object
    etap:is(embryosys_adtm_object:create("Bridge", "Alma"), {ok, alive}, "Creating Attribute Bridge.Alma"),

    % Changing the adt state
    etap:is(embryosys_adtm_object:hibern("Bridge", "Alma"), {ok, frozen}, "Freezing Attribute Bridge.Alma"),
    etap:is(embryosys_adtm_object:awake("Bridge", "Alma"), {ok, alive}, "Unfreezing Attribute Bridge.Alma"),
    etap:is(embryosys_adtm_object:destroy("Bridge", "Alma"), {ok, destroyed}, "Destroying Attribute Bridge.Alma"),
    etap:is(embryosys_adtm_object:resur("Bridge", "Alma"), {ok, alive}, "Resurecting Attribute Bridge.Alma"),

    % Try to create an object in a class that doesnt exist
    etap:is(embryosys_adtm_object:create("Nope", "Alma"), {error, invalid_parents}, "Can't create Nope.Alma"),

    % Try to recreate the same object in the same class
    etap:is(embryosys_adtm_object:create("Bridge", "Alma"), {error, already_exists}, "Can't create the same object twice"),

    application:stop(embryosys),

    etap:end_tests(),
    ok.

