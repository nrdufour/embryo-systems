#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(adtm_attribute, "Module 'adtm_attribute' loaded."),
    etap_can:has_attrib(adtm_attribute, behavior),
    etap_can:is_attrib(adtm_attribute, behavior, gen_server),
    etap_can:can_ok(adtm_attribute, create),
    etap_can:can_ok(adtm_attribute, create, 2),
    etap_can:can_ok(adtm_attribute, create, 3),
    etap_can:can_ok(adtm_attribute, hibern),
    etap_can:can_ok(adtm_attribute, hibern, 2),
    etap_can:can_ok(adtm_attribute, awake),
    etap_can:can_ok(adtm_attribute, awake, 2),
    etap_can:can_ok(adtm_attribute, destroy),
    etap_can:can_ok(adtm_attribute, destroy, 2),
    etap_can:can_ok(adtm_attribute, resur),
    etap_can:can_ok(adtm_attribute, resur, 2),
    etap_can:can_ok(adtm_attribute, purge),
    etap_can:can_ok(adtm_attribute, purge, 2),

    application:start(adtm),
    adtm_storage_server:init_storage(),

    % First create a class
    adtm_class:create("Bridge"),

    % Try to create an attribute
    etap:is(adtm_attribute:create("Bridge", "height"), {ok, alive}, "Creating Attribute Bridge.height"),

    % Changing the adt state
    etap:is(adtm_attribute:hibern("Bridge", "height"), {ok, frozen}, "Freezing Attribute Bridge.height"),
    etap:is(adtm_attribute:awake("Bridge", "height"), {ok, alive}, "Unfreezing Attribute Bridge.height"),
    etap:is(adtm_attribute:destroy("Bridge", "height"), {ok, destroyed}, "Destroying Attribute Bridge.height"),
    etap:is(adtm_attribute:resur("Bridge", "height"), {ok, alive}, "Resurecting Attribute Bridge.height"),

    % Try to create an attribute in a class that doesnt exist
    etap:is(adtm_attribute:create("Nope", "height"), {error, invalid_parents}, "Can't create Nope.height"),

    % Try to recreate the same attribute in the same class
    etap:is(adtm_attribute:create("Bridge", "height"), {error, already_exists}, "Can't create the same attribute twice"),

    application:stop(adtm),

    etap:end_tests(),
    ok.

