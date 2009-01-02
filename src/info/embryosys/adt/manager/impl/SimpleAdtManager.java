/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtFactory;
import info.embryosys.adt.core.AdtState;
import info.embryosys.adt.core.AdtType;
import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.storage.Storage;
import info.embryosys.adt.storage.impl.DummyStorage;

import java.util.List;

/**
 * @author nrdufour
 * 
 */
public class SimpleAdtManager implements AdtManager {

	private Workspace workspace;

	/**
	 * @return the workspace
	 */
	public Workspace getWorkspace() {
		return workspace;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.AdtManager#init()
	 */
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that
		// through a conf file

		this.workspace = new Workspace("default", new DummyStorage());
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.manager.AdtManager#processRequest(info.embryosys.adt
	 * .manager.AdtRequest)
	 */
	public void processRequest(final AdtRequest request) {
		Storage storage = workspace.getStorage();

		System.out.println("Processing request: " + request);
		
		// FIXME must check the existence of the parents

		switch (request.getOperation()) {
		case CREATE:
			create(storage, request.getType(), request.getArguments());
			break;
		default:
		}
	}

	public void create(final Storage storage, final AdtType type,
			final List<String> arguments) {
		// FIXME must check first if that adt exists or not.

		String name = arguments.get(arguments.size() - 1);
		Adt adt = AdtFactory.createNewAdt(type, name);
		
		adt.setState(AdtState.ALIVE);

		storage.store(adt);
	}
}
